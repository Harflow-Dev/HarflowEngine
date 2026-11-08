//! JSON text as Python's `json.dumps` writes it: default separators or `indent=2`,
//! `ensure_ascii`, and `repr` for every float. The digits are the shortest that round-trip (the standard
//! library's `{:e}`), laid out by Python's rule: an exponent when the decimal exponent is
//! below -4 or at least 16, written with a sign and at least two digits, and `.0` for an
//! integral value in fixed form.
use serde_json::{Number, Value};

/// An integer of any size written out of band: where the document has JSON pointer `pointer`
/// (RFC 6901), these decimal digits are written instead of the value found there. Only the
/// renderer that owns the document passes one; nothing in a document can ask for it.
#[derive(Clone, Copy)]
pub struct Exact<'a> {
    pub pointer: &'a str,
    pub digits: &'a str,
}

/// The JSON pointer segment of an object key.
pub(super) fn segment(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// The shortest digits that read back as `value`, in `{:e}` form. Where several decimals of
/// that length read back, Python takes the one nearest the exact value, an exact tie going to
/// the even digit; the standard formatter's shortest search rounds a tie up, so the digit
/// count it found is formatted again exactly, which rounds half to even.
fn shortest(value: f64) -> String {
    let found = format!("{value:e}");
    let mantissa = found.split_once('e').map_or(found.as_str(), |pair| pair.0);
    let count = mantissa.chars().filter(char::is_ascii_digit).count();
    let exact = format!("{:.*e}", count.saturating_sub(1), value);
    if exact.parse::<f64>().is_ok_and(|read| read == value) {
        exact
    } else {
        found
    }
}

/// Python's `repr(float)` of a finite value, and `json.dumps`' spelling of the others.
pub fn repr(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.to_string();
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0.0");
    }
    let scientific = shortest(value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    if !(-4..16).contains(&exponent) {
        let (head, tail) = digits.split_at(1);
        let fraction = if tail.is_empty() {
            String::new()
        } else {
            format!(".{tail}")
        };
        let mark = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{head}{fraction}e{mark}{:02}", exponent.abs());
    }
    if exponent < 0 {
        let zeros = "0".repeat(usize::try_from(-exponent - 1).unwrap_or(0));
        return format!("{sign}0.{zeros}{digits}");
    }
    let whole = usize::try_from(exponent).unwrap_or(0) + 1;
    if digits.len() <= whole {
        let zeros = "0".repeat(whole - digits.len());
        return format!("{sign}{digits}{zeros}.0");
    }
    let (left, right) = digits.split_at(whole);
    format!("{sign}{left}.{right}")
}

/// A JSON number as Python writes it.
pub fn number_text(number: &Number) -> String {
    match number.as_f64().filter(|_| number.is_f64()) {
        Some(float) => repr(float),
        None => number.to_string(),
    }
}

/// A string as `json.dumps` writes it with `ensure_ascii`: anything outside space to tilde is
/// escaped, as `\uXXXX` in lowercase hex and as a surrogate pair above the BMP.
fn string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(ch),
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

fn write(out: &mut String, value: &Value, indent: Option<usize>, depth: usize, at: &mut Place<'_>) {
    if let Some(digits) = at.exact_here() {
        out.push_str(digits);
        return;
    }
    let newline = |out: &mut String, level: usize| {
        if let Some(width) = indent {
            out.push('\n');
            out.push_str(&" ".repeat(width * level));
        }
    };
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => out.push_str(&number_text(number)),
        Value::String(text) => string(out, text),
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                    if indent.is_none() {
                        out.push(' ');
                    }
                }
                newline(out, depth + 1);
                let mark = at.enter(|| index.to_string());
                write(out, item, indent, depth + 1, at);
                at.leave(mark);
            }
            newline(out, depth);
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                    if indent.is_none() {
                        out.push(' ');
                    }
                }
                newline(out, depth + 1);
                string(out, key);
                out.push_str(": ");
                let mark = at.enter(|| segment(key));
                write(out, item, indent, depth + 1, at);
                at.leave(mark);
            }
            newline(out, depth);
            out.push('}');
        }
    }
}

/// Where a walk of a document stands, kept only when an out-of-band integer was asked for.
pub(super) struct Place<'a> {
    exact: Option<Exact<'a>>,
    path: String,
}

impl<'a> Place<'a> {
    pub(super) fn new(exact: Option<Exact<'a>>) -> Self {
        Self {
            exact,
            path: String::new(),
        }
    }

    /// The digits to write for the value at this place, if it is the one asked for.
    pub(super) fn exact_here(&self) -> Option<&'a str> {
        self.exact
            .filter(|exact| exact.pointer == self.path)
            .map(|exact| exact.digits)
    }

    /// Steps into a member. The segment is built only when an override is waiting.
    pub(super) fn enter(&mut self, segment: impl FnOnce() -> String) -> usize {
        let mark = self.path.len();
        if self.exact.is_some() {
            self.path.push('/');
            self.path.push_str(&segment());
        }
        mark
    }

    pub(super) fn leave(&mut self, mark: usize) {
        self.path.truncate(mark);
    }
}

/// One line as `json.dumps` writes it by default, newline-terminated.
pub fn line(value: &Value) -> String {
    line_with(value, None)
}

/// As `line`, writing `exact` where it points.
pub fn line_with(value: &Value, exact: Option<Exact<'_>>) -> String {
    let mut text = String::new();
    write(&mut text, value, None, 0, &mut Place::new(exact));
    text.push('\n');
    text
}

/// As `json.dumps(indent=2)`, newline-terminated.
pub fn pretty(value: &Value) -> String {
    pretty_with(value, None)
}

/// As `pretty`, writing `exact` where it points.
pub fn pretty_with(value: &Value, exact: Option<Exact<'_>>) -> String {
    let mut text = String::new();
    write(&mut text, value, Some(2), 0, &mut Place::new(exact));
    text.push('\n');
    text
}
