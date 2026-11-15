//! Python's `repr(float)`, and the spelling `json.dumps` gives non-finite values.
//!
//! This is a copy of `ctl/py_float.rs`'s `repr`, which is private to the ctl module. The
//! follow-up in R10-B removes this duplicate in favour of that one. The digits are the
//! shortest that round-trip (the standard library's `{:e}`), laid out by Python's rule: an
//! exponent when the decimal exponent is below -4 or at least 16, written with a sign and
//! at least two digits, and `.0` for an integral value in fixed form.

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
