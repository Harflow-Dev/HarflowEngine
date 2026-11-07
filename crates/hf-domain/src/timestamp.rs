//! Pure ISO 8601 instant parsing for stored and queried timestamps.
//!
//! An instant carries an explicit UTC offset (`Z`, `+HH:MM` or `+HHMM`); a naive value is
//! refused. The fraction has one to six digits. The result is microseconds since the Unix
//! epoch, so two spellings of one instant compare equal and offsets order correctly.
const MICROS_PER_SECOND: i64 = 1_000_000;

/// Parses `YYYY-MM-DD(T| )HH:MM:SS[.f{1,6}](Z|±HH:MM|±HHMM)`; `None` for anything else.
pub fn parse_micros(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes.len() > 32 {
        return None;
    }
    let year = digits(bytes, 0, 4)?;
    let month = digits(bytes, 5, 2)?;
    let day = digits(bytes, 8, 2)?;
    let hour = digits(bytes, 11, 2)?;
    let minute = digits(bytes, 14, 2)?;
    let second = digits(bytes, 17, 2)?;
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || !(bytes[10] == b'T' || bytes[10] == b' ')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    if !(1..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let (micros, offset) = fraction_and_offset(&bytes[19..])?;
    let days = days_from_civil(year, month, day);
    let local = days * 86_400 + hour * 3600 + minute * 60 + second;
    local
        .checked_sub(offset)?
        .checked_mul(MICROS_PER_SECOND)?
        .checked_add(micros)
}

fn fraction_and_offset(rest: &[u8]) -> Option<(i64, i64)> {
    let mut micros = 0i64;
    let mut at = 0;
    if rest.first() == Some(&b'.') {
        let count = rest[1..].iter().take_while(|b| b.is_ascii_digit()).count();
        if count == 0 || count > 6 {
            return None;
        }
        micros = digits(rest, 1, count)?;
        for _ in count..6 {
            micros *= 10;
        }
        at = 1 + count;
    }
    Some((micros, offset_seconds(&rest[at..])?))
}

fn offset_seconds(rest: &[u8]) -> Option<i64> {
    match rest {
        [b'Z'] | [b'z'] => Some(0),
        [sign @ (b'+' | b'-'), tail @ ..] => {
            let (hours, minutes) = match tail {
                [_, _, b':', _, _] => (digits(tail, 0, 2)?, digits(tail, 3, 2)?),
                [_, _, _, _] => (digits(tail, 0, 2)?, digits(tail, 2, 2)?),
                _ => return None,
            };
            if hours > 23 || minutes > 59 {
                return None;
            }
            let seconds = hours * 3600 + minutes * 60;
            Some(if *sign == b'-' { -seconds } else { seconds })
        }
        _ => None,
    }
}

fn digits(bytes: &[u8], start: usize, count: usize) -> Option<i64> {
    let slice = bytes.get(start..start.checked_add(count)?)?;
    let mut value = 0i64;
    for byte in slice {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + i64::from(byte - b'0');
    }
    Some(value)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The proleptic Gregorian date `(year, month, day)` of a day count since 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
