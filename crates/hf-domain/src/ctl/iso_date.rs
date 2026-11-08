//! The date half of the ISO 8601 reader: calendar and week dates in basic or extended form,
//! and the position of the separator that ends them.
use crate::timestamp::civil_from_days;

/// The byte at `at`, or zero past the end, as a NUL-terminated string reads.
pub(super) fn byte(text: &[u8], at: usize) -> u8 {
    text.get(at).copied().unwrap_or(0)
}

pub(super) fn is_digit(value: u8) -> bool {
    value.is_ascii_digit()
}

/// Exactly `count` ASCII digits at `at`; the value and the next position.
pub(super) fn digits(text: &[u8], at: usize, count: usize) -> Option<(i64, usize)> {
    let mut value = 0i64;
    for offset in 0..count {
        let current = byte(text, at + offset);
        if !is_digit(current) {
            return None;
        }
        value = value * 10 + i64::from(current - b'0');
    }
    Some((value, at + count))
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
pub(super) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year - era * 400;
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Monday is 0.
fn weekday(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

/// The days-since-epoch of the Monday that starts ISO week 1 of `year`.
fn week_one_monday(year: i64) -> i64 {
    let first = days_from_civil(year, 1, 1);
    let first_weekday = weekday(first);
    let monday = first - first_weekday;
    if first_weekday > 3 {
        monday + 7
    } else {
        monday
    }
}

fn week_date(year: i64, week: i64, day: i64) -> Option<i64> {
    if !(1..=9999).contains(&year) {
        return None;
    }
    if !(1..53).contains(&week) {
        let first_weekday = weekday(days_from_civil(year, 1, 1));
        let long_year = first_weekday == 3 || (first_weekday == 2 && is_leap(year));
        if week != 53 || !long_year {
            return None;
        }
    }
    if !(1..8).contains(&day) {
        return None;
    }
    Some(week_one_monday(year) + (week - 1) * 7 + day - 1)
}

/// The byte index of the date and time separator, or `None` when the string is malformed.
pub(super) fn separator(text: &[u8]) -> Option<usize> {
    let len = text.len();
    if len == 7 {
        return Some(7);
    }
    if text.get(4) == Some(&b'-') {
        if text.get(5) != Some(&b'W') {
            return Some(10);
        }
        if len < 8 {
            return None;
        }
        if len > 8 && text[8] == b'-' {
            if len == 9 {
                return None;
            }
            return Some(if len > 10 && is_digit(text[10]) {
                8
            } else {
                10
            });
        }
        return Some(8);
    }
    if text.get(4) == Some(&b'W') {
        let mut index = 7;
        while index < len && is_digit(text[index]) {
            index += 1;
        }
        if index < 9 {
            return Some(index);
        }
        return Some(if index % 2 == 0 { 7 } else { 8 });
    }
    Some(8)
}

/// The days since the epoch of the date in `text[..len]`.
pub(super) fn date(text: &[u8], len: usize) -> Option<i64> {
    let (year, mut at) = digits(text, 0, 4)?;
    let extended = byte(text, at) == b'-';
    if extended {
        at += 1;
    }
    if byte(text, at) == b'W' {
        let (week, next) = digits(text, at + 1, 2)?;
        at = next;
        let mut day = 1;
        if at < len {
            if extended {
                if byte(text, at) != b'-' {
                    return None;
                }
                at += 1;
            }
            day = digits(text, at, 1)?.0;
        }
        let days = week_date(year, week, day)?;
        let (resolved, _, _) = civil_from_days(days);
        return (1..=9999).contains(&resolved).then_some(days);
    }
    let (month, next) = digits(text, at, 2)?;
    at = next;
    if extended {
        if byte(text, at) != b'-' {
            return None;
        }
        at += 1;
    }
    let (day, _) = digits(text, at, 2)?;
    let valid = (1..=9999).contains(&year)
        && (1..=12).contains(&month)
        && day >= 1
        && day <= days_in_month(year, month);
    valid.then(|| days_from_civil(year, month, day))
}
