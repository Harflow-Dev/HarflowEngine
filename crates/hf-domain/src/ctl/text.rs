//! Text helpers shared by the ctl layers.

/// Python's `repr` of a string. ASCII and Latin-1 controls, the separators and format
/// characters listed in `NON_PRINTABLE` and the private-use area are escaped as Python does
/// (`\xNN`, `\uNNNN`, `\UNNNNNNNN`). Unassigned code points and the rarer format
/// characters are not tabulated and stay literal, which differs from `str.isprintable`.
pub fn py_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if is_non_printable(c) => escape(&mut out, c),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Non-ASCII code points that `str.isprintable` rejects, as inclusive ranges: C1 controls,
/// no-break space, soft hyphen, Arabic/Mongolian format characters, the general-punctuation
/// spaces, separators and format characters, the ideographic space, the zero-width
/// no-break space, the interlinear annotation characters and the private-use areas.
const NON_PRINTABLE: [(u32, u32); 20] = [
    (0x80, 0xa0),
    (0xad, 0xad),
    (0x600, 0x605),
    (0x61c, 0x61c),
    (0x6dd, 0x6dd),
    (0x1680, 0x1680),
    (0x180e, 0x180e),
    (0x2000, 0x200f),
    (0x2028, 0x202f),
    (0x205f, 0x2064),
    (0x2066, 0x206f),
    (0x3000, 0x3000),
    (0xe000, 0xf8ff),
    (0xfeff, 0xfeff),
    (0xfff9, 0xfffb),
    (0xf0000, 0xffffd),
    (0x100000, 0x10fffd),
    (0xe0001, 0xe0001),
    (0xe0020, 0xe007f),
    (0x110bd, 0x110bd),
];

fn is_non_printable(c: char) -> bool {
    let code = c as u32;
    code < 0x20
        || code == 0x7f
        || NON_PRINTABLE
            .iter()
            .any(|(low, high)| (*low..=*high).contains(&code))
}

fn escape(out: &mut String, c: char) {
    let code = c as u32;
    let (marker, width) = if code <= 0xff {
        ('x', 2)
    } else if code <= 0xffff {
        ('u', 4)
    } else {
        ('U', 8)
    };
    out.push('\\');
    out.push(marker);
    let digits = hex(code);
    for _ in digits.len()..width {
        out.push('0');
    }
    out.push_str(&digits);
}

fn hex(mut code: u32) -> String {
    let mut digits = Vec::new();
    while code > 0 {
        digits.push(char::from_digit(code % 16, 16).unwrap_or('0'));
        code /= 16;
    }
    if digits.is_empty() {
        digits.push('0');
    }
    digits.iter().rev().collect()
}
