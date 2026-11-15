//! The JSON reader and writers of workflow definitions: Python `json` compatibility and
//! the deliberate differences (lone surrogates, size and depth bounds).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use hf_domain::workflow::json::{Json, JsonObject};
use hf_interfaces::workflow::json_render::{
    Style, ascii_dumps_len, canonical_bytes, render, render_object,
};
use hf_interfaces::workflow::json_text::{
    INT_DIGIT_LIMIT, Limits, MAX_DEPTH, MAX_TEXT_BYTES, ParseError, parse, parse_with,
};
use hf_interfaces::workflow::py_float;

fn read(text: &str) -> Json {
    parse(text).unwrap_or_else(|error| panic!("{text:?} was refused: {error:?}"))
}

fn object(entries: &[(&str, Json)]) -> JsonObject {
    JsonObject::from_entries(
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
}

fn nested(depth: usize) -> String {
    format!("{}{}", "[".repeat(depth), "]".repeat(depth))
}

#[test]
fn duplicate_key_last_value_first_position() {
    let Json::Object(map) = read(r#"{"a":1,"b":2,"a":3}"#) else {
        panic!("not an object");
    };
    assert_eq!(map.keys().collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(map.get("a"), Some(&Json::int(3)));
    assert_eq!(map.get("b"), Some(&Json::int(2)));
}

#[test]
fn float_repr_table() {
    let table: [(f64, &str); 18] = [
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (1.0, "1.0"),
        (100.0, "100.0"),
        (1.5, "1.5"),
        (0.0001, "0.0001"),
        (1e15, "1000000000000000.0"),
        (1e16, "1e+16"),
        (1e22, "1e+22"),
        (1e-5, "1e-05"),
        (-1e-7, "-1e-07"),
        (0.00001234, "1.234e-05"),
        (5e-324, "5e-324"),
        (f64::MAX, "1.7976931348623157e+308"),
        (0.1 + 0.2, "0.30000000000000004"),
        (9_007_199_254_740_993.0, "9007199254740992.0"),
        (1.2345678901234568e17, "1.2345678901234568e+17"),
        (123456.789, "123456.789"),
    ];
    for (value, text) in table {
        assert_eq!(py_float::repr(value), text, "repr of {value:e}");
    }
    assert_eq!(py_float::repr(f64::NAN), "NaN");
    assert_eq!(py_float::repr(f64::INFINITY), "Infinity");
    assert_eq!(py_float::repr(f64::NEG_INFINITY), "-Infinity");
}

#[test]
fn ensure_ascii_astral_pairs() {
    let text = Json::text("é😀");
    assert_eq!(render(&text, Style::Line), r#""\u00e9\ud83d\ude00""#);
    assert_eq!(render(&text, Style::Canonical), r#""\u00e9\ud83d\ude00""#);
    assert_eq!(render(&text, Style::Pretty), r#""\u00e9\ud83d\ude00""#);
    assert_eq!(ascii_dumps_len(&text), 20);
}

#[test]
fn escapes_0x7f() {
    let text = Json::text("a\u{7f}b");
    assert_eq!(render(&text, Style::Line), r#""a\u007fb""#);
    assert_eq!(render(&text, Style::Canonical), r#""a\u007fb""#);
    assert_eq!(render(&text, Style::Storage), "\"a\u{7f}b\"");
}

#[test]
fn sorted_keys_code_point() {
    let map = object(&[
        ("\u{1f600}", Json::int(1)),
        ("\u{ffff}", Json::int(2)),
        ("a", Json::int(3)),
        ("B", Json::int(4)),
        ("", Json::int(5)),
    ]);
    // Code point order, not UTF-16 order: U+FFFF sorts before U+1F600.
    assert_eq!(
        String::from_utf8(canonical_bytes(&map)).unwrap(),
        r#"{"":5,"B":4,"a":3,"\uffff":2,"\ud83d\ude00":1}"#
    );
    assert_eq!(
        render_object(&map, Style::Line),
        r#"{"\ud83d\ude00": 1, "\uffff": 2, "a": 3, "B": 4, "": 5}"#
    );
}

#[test]
fn canonical_sorts_nested_objects() {
    let inner = object(&[("z", Json::int(1)), ("y", Json::Null)]);
    let map = object(&[("b", Json::Object(inner)), ("a", Json::Bool(true))]);
    assert_eq!(
        String::from_utf8(canonical_bytes(&map)).unwrap(),
        r#"{"a":true,"b":{"y":null,"z":1}}"#
    );
}

#[test]
fn big_int_digits_preserved() {
    let big = "123456789012345678901234567890123456789012345678901234567890";
    assert_eq!(read(big), Json::Int(parse_int(big)));
    assert_eq!(render(&read(big), Style::Line), big);
    let negative = format!("-{big}");
    assert_eq!(render(&read(&negative), Style::Canonical), negative);
    assert_eq!(render(&read("-0"), Style::Line), "0");
    assert!(matches!(read("1e2"), Json::Float(value) if value == 100.0));
    assert!(matches!(read("-0.0"), Json::Float(value) if value == 0.0 && value.is_sign_negative()));
}

fn parse_int(digits: &str) -> hf_domain::workflow::json::JsonInt {
    hf_domain::workflow::json::JsonInt::parse(false, digits).unwrap()
}

#[test]
fn int_digit_limit() {
    let at_limit = "9".repeat(INT_DIGIT_LIMIT);
    assert!(matches!(read(&at_limit), Json::Int(_)));
    let over = "9".repeat(INT_DIGIT_LIMIT + 1);
    assert_eq!(parse(&over), Err(ParseError::IntegerTooLong));
    assert_eq!(parse(&format!("-{over}")), Err(ParseError::IntegerTooLong));
    assert_eq!(parse(&format!("[{over}]")), Err(ParseError::IntegerTooLong));
}

#[test]
fn lone_surrogate_refused() {
    for text in [
        r#""\ud800""#,
        r#""\udc00""#,
        r#""\udbff""#,
        r#""\ud800x""#,
        r#""\ud800\u0041""#,
        r#""\ud800\ud800""#,
        r#""\udc00\ud800""#,
    ] {
        assert_eq!(parse(text), Err(ParseError::LoneSurrogate), "{text}");
    }
    assert_eq!(parse(r#"{"k\ud800":1}"#), Err(ParseError::LoneSurrogate));
    assert_eq!(read(r#""\ud83d\ude00""#), Json::text("\u{1f600}"));
    assert_eq!(read(r#""\uD83D\uDE00""#), Json::text("\u{1f600}"));
}

#[test]
fn nan_tokens_parse() {
    assert!(matches!(read("NaN"), Json::Float(value) if value.is_nan()));
    assert!(matches!(read("Infinity"), Json::Float(value) if value == f64::INFINITY));
    assert!(matches!(read("-Infinity"), Json::Float(value) if value == f64::NEG_INFINITY));
    let Json::Array(items) = read("[NaN, -Infinity]") else {
        panic!("not an array");
    };
    assert_eq!(items.len(), 2);
    assert!(matches!(read("1e999"), Json::Float(value) if value == f64::INFINITY));
    for text in ["nan", "inf", "-NaN", "+Infinity", "infinity"] {
        assert!(matches!(parse(text), Err(ParseError::Syntax(_))), "{text}");
    }
    assert_eq!(render(&Json::Float(f64::NAN), Style::Line), "NaN");
    assert_eq!(
        render(&Json::Float(f64::NEG_INFINITY), Style::Line),
        "-Infinity"
    );
}

#[test]
fn depth_bound() {
    assert!(parse(&nested(MAX_DEPTH)).is_ok());
    assert_eq!(parse(&nested(MAX_DEPTH + 1)), Err(ParseError::TooDeep));
    let objects = format!("{}1{}", r#"{"a":"#.repeat(MAX_DEPTH), "}".repeat(MAX_DEPTH));
    assert!(parse(&objects).is_ok());
    let too_deep = format!(
        "{}1{}",
        r#"{"a":"#.repeat(MAX_DEPTH + 1),
        "}".repeat(MAX_DEPTH + 1)
    );
    assert_eq!(parse(&too_deep), Err(ParseError::TooDeep));
    let limits = Limits {
        max_bytes: usize::MAX,
        max_depth: 2,
    };
    assert!(parse_with("[[1]]", limits).is_ok());
    assert_eq!(parse_with("[[[1]]]", limits), Err(ParseError::TooDeep));
}

#[test]
fn text_size_bound() {
    let filler = "a".repeat(MAX_TEXT_BYTES - 2);
    assert!(parse(&format!("\"{filler}\"")).is_ok());
    assert_eq!(parse(&format!("\"{filler}a\"")), Err(ParseError::TooLarge));
    let loose = Limits {
        max_bytes: MAX_TEXT_BYTES + 1,
        max_depth: MAX_DEPTH,
    };
    assert!(parse_with(&format!("\"{filler}a\""), loose).is_ok());
}

#[test]
fn storage_mode_non_ascii() {
    let map = object(&[
        ("é", Json::text("😀\u{2028}")),
        ("c", Json::text("\u{1}\n\"\\")),
    ]);
    assert_eq!(
        render_object(&map, Style::Storage),
        "{\"é\": \"😀\u{2028}\", \"c\": \"\\u0001\\n\\\"\\\\\"}"
    );
    assert_eq!(
        render_object(&map, Style::Line),
        r#"{"\u00e9": "\ud83d\ude00\u2028", "c": "\u0001\n\"\\"}"#
    );
}

#[test]
fn line_and_pretty_layout() {
    let value = read(r#"{"a":[1,2.5,null],"b":{},"c":[],"d":{"e":true}}"#);
    assert_eq!(
        render(&value, Style::Line),
        r#"{"a": [1, 2.5, null], "b": {}, "c": [], "d": {"e": true}}"#
    );
    assert_eq!(
        render(&value, Style::Canonical),
        r#"{"a":[1,2.5,null],"b":{},"c":[],"d":{"e":true}}"#
    );
    assert_eq!(
        render(&value, Style::Pretty),
        "{\n  \"a\": [\n    1,\n    2.5,\n    null\n  ],\n  \"b\": {},\n  \"c\": [],\n  \"d\": {\n    \"e\": true\n  }\n}"
    );
    assert_eq!(render(&Json::Array(Vec::new()), Style::Pretty), "[]");
}

#[test]
fn whitespace_and_syntax_errors() {
    assert_eq!(read(" \t\r\n[ 1 ,\n2 ]\n"), read("[1,2]"));
    for text in [
        "",
        "[1,]",
        "{\"a\":1,}",
        "01",
        "1.",
        ".5",
        "+1",
        "'a'",
        "[1 2]",
        "{\"a\" 1}",
        "{1:2}",
        "\"a",
        "\"\\x\"",
        "\"\\u12\"",
        "tru",
        "[1]x",
        "\"a\nb\"",
        "\"a\tb\"",
        "\u{a0}1",
        "--1",
        "1e",
        "[",
    ] {
        assert!(
            matches!(parse(text), Err(ParseError::Syntax(_))),
            "{text:?}"
        );
    }
}

#[test]
fn escapes_round_trip() {
    let value = read(r#""\"\\\/\b\f\n\r\t\u0041""#);
    assert_eq!(value, Json::text("\"\\/\u{8}\u{c}\n\r\tA"));
    assert_eq!(render(&value, Style::Line), r#""\"\\/\b\f\n\r\tA""#);
}
