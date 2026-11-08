//! Time values of the statistics page: instants as seconds, the bounds of a window and the
//! retention policy the page reports.
use super::{iso_parse::aware_micros, py_int::parse};

const MICROS: u64 = 1_000_000;

/// The seconds of an instant given in microseconds: the correctly rounded value of
/// `micros / 10**6`, which is what dividing a whole number of microseconds gives. The value
/// is written as a signed decimal and parsed, so no intermediate rounding occurs.
pub fn micros_to_seconds(micros: i64) -> f64 {
    let magnitude = micros.unsigned_abs();
    let mut text = String::new();
    if micros < 0 {
        text.push('-');
    }
    text.push_str(&(magnitude / MICROS).to_string());
    text.push('.');
    let fraction = (magnitude % MICROS).to_string();
    for _ in fraction.len()..6 {
        text.push('0');
    }
    text.push_str(&fraction);
    text.parse().unwrap_or(0.0)
}

/// An ISO 8601 bound with an explicit offset, in microseconds. Every `Z` is read as
/// `+00:00` first, as the reference does, so a lowercase `z` is not an offset.
pub fn bound_micros(text: &str) -> Option<i64> {
    aware_micros(&text.replace('Z', "+00:00"))
}

/// A window `since <= start < before`, as the microseconds the cursor binds and the seconds
/// the comparisons use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub since_us: i64,
    pub before_us: i64,
    pub since: f64,
    pub before: f64,
}

/// `None` unless both bounds are qualified instants and `since` is below `before`, compared
/// as seconds.
pub fn bounds(since: &str, before: &str) -> Option<Bounds> {
    let since_us = bound_micros(since)?;
    let before_us = bound_micros(before)?;
    let (low, high) = (micros_to_seconds(since_us), micros_to_seconds(before_us));
    (low < high).then_some(Bounds {
        since_us,
        before_us,
        since: low,
        before: high,
    })
}

/// The retention policy as `HARFLOW_RETENTION_DAYS` states it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Retention {
    /// A negative or non-integer value.
    Unknown,
    Disabled,
    /// A positive whole number of days of any size, as canonical decimal text.
    Days(String),
}

/// The retention the page reports. Unset is 30 days; a negative or unreadable value is
/// unknown; zero disables the sweep.
pub fn retention(raw: Option<&str>) -> Retention {
    let Some(raw) = raw else {
        return Retention::Days("30".to_string());
    };
    match parse(raw) {
        None => Retention::Unknown,
        Some(value) if value.negative => Retention::Unknown,
        Some(value) if value.digits == "0" => Retention::Disabled,
        Some(value) => Retention::Days(value.digits),
    }
}
