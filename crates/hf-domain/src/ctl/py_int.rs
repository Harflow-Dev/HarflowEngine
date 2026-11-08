//! Python's `int(text)` for a decimal string, exactly: surrounding Unicode white space, one
//! sign, decimal digits of any script with single underscores between digits, and the
//! interpreter's limit of 4,300 digits. (Unlike `str.strip`, `int` does not skip the
//! separators U+001C to U+001F; that was checked against CPython 3.13.5.)

/// The most digits `int` reads in a decimal string.
pub const DIGIT_LIMIT: usize = 4300;

/// The code point of the digit zero of each run of ten decimal digits (category Nd). Generated
/// once from CPython 3.13.5 (Unicode 15.1): every `cp` with `unicodedata.category(chr(cp)) ==
/// "Nd"` and `unicodedata.decimal(chr(cp)) == 0`, each checked to start a run of ten.
const ZEROS: [u32; 68] = [
    0x30, 0x660, 0x6F0, 0x7C0, 0x966, 0x9E6, 0xA66, 0xAE6, 0xB66, 0xBE6, 0xC66, 0xCE6, 0xD66,
    0xDE6, 0xE50, 0xED0, 0xF20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90,
    0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10,
    0x104A0, 0x10D30, 0x11066, 0x110F0, 0x11136, 0x111D0, 0x112F0, 0x11450, 0x114D0, 0x11650,
    0x116C0, 0x11730, 0x118E0, 0x11950, 0x11C50, 0x11D50, 0x11DA0, 0x11F50, 0x16A60, 0x16AC0,
    0x16B50, 0x1D7CE, 0x1D7D8, 0x1D7E2, 0x1D7EC, 0x1D7F6, 0x1E140, 0x1E2F0, 0x1E4F0, 0x1E950,
    0x1FBF0,
];

/// An integer of any size: its sign and canonical decimal digits (no leading zeros; `0` is
/// never negative).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PyInt {
    pub negative: bool,
    pub digits: String,
}

fn digit(ch: char) -> Option<u32> {
    let code = ch as u32;
    ZEROS
        .iter()
        .find(|zero| (**zero..**zero + 10).contains(&code))
        .map(|zero| code - *zero)
}

/// `None` where `int(text)` raises `ValueError`.
pub fn parse(text: &str) -> Option<PyInt> {
    let trimmed = text.trim();
    let (negative, body) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let mut digits = String::new();
    let mut after_digit = false;
    for ch in body.chars() {
        if ch == '_' {
            if !after_digit {
                return None;
            }
            after_digit = false;
            continue;
        }
        let value = digit(ch)?;
        digits.push(char::from_digit(value, 10)?);
        after_digit = true;
    }
    if digits.is_empty() || !after_digit || digits.len() > DIGIT_LIMIT {
        return None;
    }
    let stripped = digits.trim_start_matches('0');
    let canonical = if stripped.is_empty() { "0" } else { stripped };
    Some(PyInt {
        negative: negative && canonical != "0",
        digits: canonical.to_string(),
    })
}

impl PyInt {
    /// The value, saturated at the `i64` range.
    pub fn saturating(&self) -> i64 {
        match self.digits.parse::<i64>() {
            Ok(value) if self.negative => -value,
            Ok(value) => value,
            Err(_) if self.negative => i64::MIN,
            Err(_) => i64::MAX,
        }
    }
}
