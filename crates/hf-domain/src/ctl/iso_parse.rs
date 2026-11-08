//! A pure port of CPython 3.13's `datetime.fromisoformat` for bounds that must carry an
//! explicit offset.
//!
//! The grammar is the one the interpreter's C implementation reads: calendar, ordinal-free
//! and week dates in basic or extended form, any one separator character, `HH[:MM[:SS]]`
//! with an optional comma or dot fraction truncated to microseconds, and an offset of
//! `HH[:MM[:SS[.f]]]`. The caller replaces `Z` first. The result is microseconds since the
//! Unix epoch, computed without building a UTC date, so an offset that crosses year 9999 or
//! year 1 still resolves.
use super::iso_date::{byte, date, digits, is_digit, separator};

const MICROS: i64 = 1_000_000;

/// `HH[:?MM[:?SS[{.,}f+]]]` over `text[start..end]`. Returns the components, the
/// microseconds and whether text is left over after the fraction.
struct Clock {
    hour: i64,
    minute: i64,
    second: i64,
    micro: i64,
    leftover: bool,
}

fn clock(text: &[u8], start: usize, end: usize) -> Option<Clock> {
    let mut parts = [0i64; 3];
    let mut at = start;
    let mut extended = true;
    for index in 0..3 {
        let (value, next) = digits(text, at, 2)?;
        parts[index] = value;
        let current = byte(text, next);
        at = next + 1;
        if index == 0 {
            extended = current == b':';
        }
        if at >= end {
            return Some(Clock {
                hour: parts[0],
                minute: parts[1],
                second: parts[2],
                micro: 0,
                leftover: current != 0,
            });
        } else if extended && current == b':' {
            continue;
        } else if current == b'.' || current == b',' {
            break;
        } else if !extended {
            at -= 1;
        } else {
            return None;
        }
    }
    // The interpreter reads a fraction after any third separator as well, so a stray
    // `HH:MM:SS:f` is a fraction of one digit.
    let take = end.saturating_sub(at).min(6);
    let (mut micro, mut next) = digits(text, at, take)?;
    for _ in take..6 {
        micro *= 10;
    }
    while is_digit(byte(text, next)) {
        next += 1;
    }
    let leftover = byte(text, next) != 0;
    Some(Clock {
        hour: parts[0],
        minute: parts[1],
        second: parts[2],
        micro,
        leftover,
    })
}

/// The offset in microseconds of an explicit zone, when the time carried one.
struct Parsed {
    clock: Clock,
    offset: Option<i64>,
}

fn time(text: &[u8], start: usize) -> Option<Parsed> {
    let end = text.len();
    let mut zone = start;
    while zone < end && !matches_zone(text[zone]) {
        zone += 1;
    }
    if start >= end {
        zone = end;
    }
    let local = clock(text, start, zone)?;
    if zone == end {
        return if local.leftover {
            None
        } else {
            Some(Parsed {
                clock: local,
                offset: None,
            })
        };
    }
    if text[zone] == b'Z' {
        return (zone + 1 == end).then_some(Parsed {
            clock: local,
            offset: Some(0),
        });
    }
    let sign = if text[zone] == b'-' { -1 } else { 1 };
    let zone_clock = clock(text, zone + 1, end)?;
    if zone_clock.leftover {
        return None;
    }
    let seconds = zone_clock.hour * 3600 + zone_clock.minute * 60 + zone_clock.second;
    let signed = sign * seconds;
    // The interpreter reads a zero whole-second offset as UTC whatever its microseconds.
    let offset = if signed == 0 {
        0
    } else {
        signed * MICROS + sign * zone_clock.micro
    };
    (offset.abs() < 86_400 * MICROS).then_some(Parsed {
        clock: local,
        offset: Some(offset),
    })
}

fn matches_zone(value: u8) -> bool {
    value == b'Z' || value == b'+' || value == b'-'
}

/// The instant of `text` in microseconds since the epoch; `None` when the string is not a
/// valid `fromisoformat` input, carries no offset, or names no real date or time.
pub fn aware_micros(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 7 {
        return None;
    }
    let split = separator(bytes)?;
    let days = date(bytes, split)?;
    let mut parsed = Parsed {
        clock: Clock {
            hour: 0,
            minute: 0,
            second: 0,
            micro: 0,
            leftover: false,
        },
        offset: None,
    };
    if bytes.len() > split {
        let lead = bytes[split];
        let width = if lead & 0x80 == 0 {
            1
        } else {
            match lead & 0xf0 {
                0xe0 => 3,
                0xf0 => 4,
                _ => 2,
            }
        };
        parsed = time(bytes, split + width)?;
    }
    let clock = parsed.clock;
    if clock.hour > 23 || clock.minute > 59 || clock.second > 59 {
        return None;
    }
    let offset = parsed.offset?;
    let seconds = days * 86_400 + clock.hour * 3600 + clock.minute * 60 + clock.second;
    seconds
        .checked_mul(MICROS)?
        .checked_add(clock.micro)?
        .checked_sub(offset)
}
