//! The text of cursor JSON bytes as Python decodes it: the encoding `json.detect_encoding`
//! finds, then the codec with `surrogatepass`, so lone surrogates survive as code points.

fn code(unit: u16) -> u32 {
    u32::from(unit)
}

fn units(bytes: &[u8], big: bool) -> Option<Vec<u32>> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let raw: Vec<u16> = bytes
        .chunks(2)
        .map(|pair| {
            if big {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        let unit = raw[at];
        let low = raw
            .get(at + 1)
            .copied()
            .filter(|next| (0xDC00..0xE000).contains(next));
        if (0xD800..0xDC00).contains(&unit)
            && let Some(low) = low
        {
            out.push(0x10000 + ((code(unit) - 0xD800) << 10) + (code(low) - 0xDC00));
            at += 2;
        } else {
            out.push(code(unit));
            at += 1;
        }
    }
    Some(out)
}

fn words(bytes: &[u8], big: bool) -> Option<Vec<u32>> {
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    bytes
        .chunks(4)
        .map(|quad| {
            let array = [quad[0], quad[1], quad[2], quad[3]];
            let value = if big {
                u32::from_be_bytes(array)
            } else {
                u32::from_le_bytes(array)
            };
            (value <= 0x10FFFF).then_some(value)
        })
        .collect()
}

/// UTF-8 with surrogate code points allowed, and nothing overlong.
fn utf8(bytes: &[u8]) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let lead = bytes[at];
        let (extra, minimum, start) = match lead {
            0x00..=0x7F => (0, 0, u32::from(lead)),
            0xC2..=0xDF => (1, 0x80, u32::from(lead & 0x1F)),
            0xE0..=0xEF => (2, 0x800, u32::from(lead & 0x0F)),
            0xF0..=0xF4 => (3, 0x10000, u32::from(lead & 0x07)),
            _ => return None,
        };
        let tail = bytes.get(at + 1..at + 1 + extra)?;
        let mut value = start;
        for byte in tail {
            if byte & 0xC0 != 0x80 {
                return None;
            }
            value = (value << 6) | u32::from(byte & 0x3F);
        }
        if value < minimum || value > 0x10FFFF {
            return None;
        }
        out.push(value);
        at += 1 + extra;
    }
    Some(out)
}

/// `json.detect_encoding`, then the codec with `surrogatepass`.
pub(super) fn decode(bytes: &[u8]) -> Option<Vec<u32>> {
    let starts = |prefix: &[u8]| bytes.starts_with(prefix);
    if starts(&[0xFF, 0xFE, 0, 0]) {
        return words(&bytes[4..], false);
    }
    if starts(&[0, 0, 0xFE, 0xFF]) {
        return words(&bytes[4..], true);
    }
    if starts(&[0xFF, 0xFE]) {
        return units(&bytes[2..], false);
    }
    if starts(&[0xFE, 0xFF]) {
        return units(&bytes[2..], true);
    }
    if starts(&[0xEF, 0xBB, 0xBF]) {
        return utf8(&bytes[3..]);
    }
    if bytes.len() >= 4 {
        if bytes[0] == 0 {
            return if bytes[1] != 0 {
                units(bytes, true)
            } else {
                words(bytes, true)
            };
        }
        if bytes[1] == 0 {
            let narrow = bytes[2] != 0 || bytes[3] != 0;
            return if narrow {
                units(bytes, false)
            } else {
                words(bytes, false)
            };
        }
    } else if bytes.len() == 2 {
        if bytes[0] == 0 {
            return units(bytes, true);
        }
        if bytes[1] == 0 {
            return units(bytes, false);
        }
    }
    utf8(bytes)
}
