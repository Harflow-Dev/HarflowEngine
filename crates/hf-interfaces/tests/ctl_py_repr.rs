//! `py_repr` follows Python's `repr` for strings.
use hf_domain::ctl::text::py_repr;

#[test]
fn controls_and_quotes_follow_python() {
    assert_eq!(py_repr("a\u{0}b\u{1f}\u{7f}"), "'a\\x00b\\x1f\\x7f'");
    assert_eq!(py_repr("t\tn\nr\r"), "'t\\tn\\nr\\r'");
    assert_eq!(py_repr("it's"), "\"it's\"");
    assert_eq!(py_repr("it's \"x\""), "'it\\'s \"x\"'");
    assert_eq!(py_repr("back\\slash"), "'back\\\\slash'");
}

#[test]
fn non_printable_non_ascii_uses_the_python_escape_widths() {
    assert_eq!(py_repr("\u{80}\u{a0}\u{ad}"), "'\\x80\\xa0\\xad'");
    assert_eq!(
        py_repr("\u{2028}\u{feff}\u{e000}"),
        "'\\u2028\\ufeff\\ue000'"
    );
    assert_eq!(py_repr("\u{f0000}"), "'\\U000f0000'");
    assert_eq!(py_repr("caf\u{e9} \u{1F600}"), "'caf\u{e9} \u{1F600}'");
}
