//! Integers of any size, as Python's `int`.
use std::cmp::Ordering;

/// An integer of any size: a sign and canonical decimal digits (no leading zeros; zero is
/// never negative).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JsonInt {
    negative: bool,
    digits: String,
}

/// Smallest integer magnitude that Python's `float()` refuses (`2**1024 - 2**970`).
const FLOAT_OVERFLOW: &str = "179769313486231580793728971405303415079934132710037826936173778980444968292764750946649017977587207096330286416692887910946555547851940402630657488671505820681908902000708383676273854845817711531764475730270069855571366959622842914819860834936475292719074168444365510704342711559699508093042880177904174497792";

fn compare_magnitude(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn add_one(digits: &str) -> String {
    let mut out: Vec<u8> = digits.bytes().map(|byte| byte - b'0').collect();
    let mut index = out.len();
    loop {
        if index == 0 {
            out.insert(0, 1);
            break;
        }
        index -= 1;
        if out[index] == 9 {
            out[index] = 0;
        } else {
            out[index] += 1;
            break;
        }
    }
    out.iter().map(|digit| char::from(b'0' + digit)).collect()
}

/// One less than a positive magnitude.
fn sub_one(digits: &str) -> String {
    let mut out: Vec<u8> = digits.bytes().map(|byte| byte - b'0').collect();
    let mut index = out.len();
    while index > 0 {
        index -= 1;
        if out[index] == 0 {
            out[index] = 9;
        } else {
            out[index] -= 1;
            break;
        }
    }
    let first = out
        .iter()
        .position(|digit| *digit != 0)
        .unwrap_or(out.len() - 1);
    out[first..]
        .iter()
        .map(|digit| char::from(b'0' + digit))
        .collect()
}

impl JsonInt {
    pub fn from_i64(value: i64) -> Self {
        Self {
            negative: value < 0,
            digits: value.unsigned_abs().to_string(),
        }
    }

    /// `None` unless `digits` is a nonempty run of ASCII digits. Leading zeros and a
    /// negative zero are normalized away.
    pub fn parse(negative: bool, digits: &str) -> Option<Self> {
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let trimmed = digits.trim_start_matches('0');
        let magnitude = if trimmed.is_empty() { "0" } else { trimmed };
        Some(Self {
            negative: negative && magnitude != "0",
            digits: magnitude.to_string(),
        })
    }

    pub fn is_negative(&self) -> bool {
        self.negative
    }

    pub fn is_zero(&self) -> bool {
        self.digits == "0"
    }

    /// The decimal digits without sign.
    pub fn digits(&self) -> &str {
        &self.digits
    }

    /// The decimal text, with a leading `-` when negative.
    pub fn text(&self) -> String {
        if self.negative {
            let mut out = String::from("-");
            out.push_str(&self.digits);
            out
        } else {
            self.digits.clone()
        }
    }

    pub fn to_i64(&self) -> Option<i64> {
        let magnitude: i128 = self.digits.parse().ok()?;
        i64::try_from(if self.negative { -magnitude } else { magnitude }).ok()
    }

    /// This value plus one, at any magnitude.
    pub fn increment(&self) -> Self {
        if self.negative {
            let digits = sub_one(&self.digits);
            Self {
                negative: digits != "0",
                digits,
            }
        } else {
            Self {
                negative: false,
                digits: add_one(&self.digits),
            }
        }
    }

    /// Whether Python's `float(self)` raises `OverflowError`.
    pub fn exceeds_float(&self) -> bool {
        compare_magnitude(&self.digits, FLOAT_OVERFLOW) != Ordering::Less
    }

    /// Python's `float(self)` for a value that does not overflow, `None` otherwise.
    pub fn to_float(&self) -> Option<f64> {
        if self.exceeds_float() {
            return None;
        }
        self.text().parse().ok()
    }
}

impl Ord for JsonInt {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => compare_magnitude(&self.digits, &other.digits),
            (true, true) => compare_magnitude(&other.digits, &self.digits),
        }
    }
}

impl PartialOrd for JsonInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn int(negative: bool, digits: &str) -> JsonInt {
        JsonInt::parse(negative, digits).unwrap()
    }

    #[test]
    fn int_text_normalizes_negative_zero() {
        assert_eq!(int(true, "000").text(), "0");
        assert!(!int(true, "0").is_negative());
        assert_eq!(int(true, "0012").text(), "-12");
        assert!(JsonInt::parse(false, "").is_none());
        assert!(JsonInt::parse(false, "1x").is_none());
    }

    #[test]
    fn int_increment_beyond_i64() {
        let top = JsonInt::from_i64(i64::MAX);
        let next = top.increment();
        assert_eq!(next.text(), "9223372036854775808");
        assert_eq!(next.to_i64(), None);
        assert_eq!(int(false, "999").increment().text(), "1000");
        assert_eq!(int(true, "1").increment().text(), "0");
        assert_eq!(int(true, "10").increment().text(), "-9");
        assert_eq!(JsonInt::from_i64(i64::MIN).to_i64(), Some(i64::MIN));
    }

    #[test]
    fn int_cmp_mixed_sign() {
        assert!(int(true, "5") < int(false, "0"));
        assert!(int(true, "10") < int(true, "9"));
        assert!(int(false, "10") > int(false, "9"));
        assert!(int(false, "123456789012345678901234567890") > JsonInt::from_i64(i64::MAX));
    }

    #[test]
    fn float_overflow_boundary() {
        assert!(int(false, FLOAT_OVERFLOW).exceeds_float());
        assert!(!int(false, "1000000000000000000000").exceeds_float());
        assert_eq!(int(false, "10").to_float(), Some(10.0));
    }
}
