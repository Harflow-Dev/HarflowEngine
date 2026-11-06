//! Iterative, bounded scan of one JSON object. It checks that the whole input is valid
//! UTF-8 and well-formed JSON (string escapes, number and literal syntax, balanced
//! containers, nesting up to a fixed depth) without recursion, so deeply nested valid
//! documents are not rejected by a parser recursion limit. Nothing is stored per member
//! of an arbitrary document: only the byte ranges of a fixed set of journal fields are
//! kept, and the names directly under `inventory` are checked as they are scanned.
const MAX_DEPTH: usize = 4096;
/// Top-level members whose value range is kept.
const TOP_KEYS: [&str; 10] = [
    "operation_id",
    "phase",
    "product",
    "format",
    "v",
    "state_root",
    "proposal",
    "backup_path",
    "root_identity",
    "inventory",
];
const PROPOSAL_INDEX: usize = 6;
const INVENTORY_INDEX: usize = 9;
/// Members of `proposal` whose value range is kept.
const PROPOSAL_KEYS: [&str; 3] = ["operation_id", "state_root", "root_identity"];
#[derive(Clone, Copy, Default)]
struct Slot {
    count: u32,
    start: usize,
    end: usize,
}
impl Slot {
    fn note(&mut self, start: usize, end: usize) {
        self.count = self.count.saturating_add(1);
        self.start = start;
        self.end = end;
    }
    /// A repeated key qualifies nothing.
    fn once(&self) -> Option<(usize, usize)> {
        (self.count == 1).then_some((self.start, self.end))
    }
}
pub struct Scan {
    top: [Slot; TOP_KEYS.len()],
    proposal: [Slot; PROPOSAL_KEYS.len()],
    names_ok: bool,
}
impl Scan {
    pub fn top(&self, key: &str) -> Option<(usize, usize)> {
        self.top
            .get(TOP_KEYS.iter().position(|k| *k == key)?)?
            .once()
    }
    pub fn proposal(&self, key: &str) -> Option<(usize, usize)> {
        self.proposal
            .get(PROPOSAL_KEYS.iter().position(|k| *k == key)?)?
            .once()
    }
    /// Every member name directly under a top-level `inventory` object passed the check.
    pub fn inventory_names_valid(&self) -> bool {
        self.names_ok
    }
}
enum Frame {
    Object {
        begin: usize,
        key: Option<(usize, usize)>,
        next: Next,
        /// Index of the current key in the key list that applies at this level.
        tag: Option<usize>,
    },
    Array {
        begin: usize,
        next: Next,
    },
}
#[derive(Clone, Copy, PartialEq)]
enum Next {
    /// First position: a value, or the closing bracket.
    First,
    Key,
    Colon,
    Value,
    CommaOrEnd,
}
pub fn decode_string(raw: &[u8]) -> Option<String> {
    serde_json::from_slice::<String>(raw).ok()
}
fn skip(bytes: &[u8], mut pos: usize) -> usize {
    while matches!(bytes.get(pos), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        pos += 1;
    }
    pos
}
/// Returns the position after the closing quote; escapes are checked.
fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut pos = start + 1;
    loop {
        match *bytes.get(pos)? {
            b'"' => return Some(pos + 1),
            b'\\' => match *bytes.get(pos + 1)? {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => pos += 2,
                b'u' => {
                    let digits = bytes.get(pos + 2..pos + 6)?;
                    if !digits.iter().all(u8::is_ascii_hexdigit) {
                        return None;
                    }
                    pos += 6;
                }
                _ => return None,
            },
            0..=0x1f => return None,
            _ => pos += 1,
        }
    }
}
fn scalar_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut end = start;
    while let Some(byte) = bytes.get(end) {
        if matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b',' | b']' | b'}') {
            break;
        }
        end += 1;
    }
    let token = bytes.get(start..end)?;
    (matches!(token, b"true" | b"false" | b"null") || is_number(token)).then_some(end)
}
fn is_number(token: &[u8]) -> bool {
    let digits = |from: usize| {
        token.get(from..).map_or(0, |rest| {
            rest.iter().take_while(|b| b.is_ascii_digit()).count()
        })
    };
    let mut at = usize::from(token.first() == Some(&b'-'));
    let whole = digits(at);
    if whole == 0 || (whole > 1 && token.get(at) == Some(&b'0')) {
        return false;
    }
    at += whole;
    if token.get(at) == Some(&b'.') {
        let fraction = digits(at + 1);
        if fraction == 0 {
            return false;
        }
        at += 1 + fraction;
    }
    if matches!(token.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(token.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exponent = digits(at);
        if exponent == 0 {
            return false;
        }
        at += exponent;
    }
    at == token.len()
}
/// One step: the new position and, when a value finished, its byte range.
type Step = Option<(usize, Option<(usize, usize)>)>;
pub fn scan_object(bytes: &[u8], valid_name: fn(&str) -> bool) -> Option<Scan> {
    std::str::from_utf8(bytes).ok()?;
    let mut stack: Vec<Frame> = Vec::new();
    let mut scan = Scan {
        top: Default::default(),
        proposal: Default::default(),
        names_ok: true,
    };
    let mut pos = skip(bytes, 0);
    if bytes.get(pos) != Some(&b'{') {
        return None;
    }
    loop {
        pos = skip(bytes, pos);
        let byte = *bytes.get(pos)?;
        let wants_value = match stack.last() {
            None => true,
            Some(Frame::Object { next, .. }) => *next == Next::Value,
            Some(Frame::Array { next, .. }) => matches!(next, Next::First | Next::Value),
        };
        let (after, completed) = if wants_value {
            step_value(bytes, &mut stack, pos, byte)?
        } else {
            step_punctuation(bytes, &mut stack, &mut scan, valid_name, pos, byte)?
        };
        pos = after;
        if let Some((start, end)) = completed {
            if stack.is_empty() {
                return (skip(bytes, pos) == bytes.len()).then_some(scan);
            }
            record(&mut stack, &mut scan, start, end)?;
        }
    }
}
fn step_value(bytes: &[u8], stack: &mut Vec<Frame>, pos: usize, byte: u8) -> Step {
    match byte {
        b'{' | b'[' => {
            if stack.len() >= MAX_DEPTH {
                return None;
            }
            stack.push(if byte == b'{' {
                Frame::Object {
                    begin: pos,
                    key: None,
                    next: Next::First,
                    tag: None,
                }
            } else {
                Frame::Array {
                    begin: pos,
                    next: Next::First,
                }
            });
            Some((pos + 1, None))
        }
        b']' if matches!(
            stack.last(),
            Some(Frame::Array {
                next: Next::First,
                ..
            })
        ) =>
        {
            Some((pos + 1, Some((close(stack)?, pos + 1))))
        }
        b'"' => {
            let end = string_end(bytes, pos)?;
            Some((end, Some((pos, end))))
        }
        _ => {
            let end = scalar_end(bytes, pos)?;
            Some((end, Some((pos, end))))
        }
    }
}
/// Classifies a key at depth 1 or 2 against the kept keys; deeper keys are ignored.
fn key_tag(
    bytes: &[u8],
    stack: &[Frame],
    range: (usize, usize),
    scan: &mut Scan,
    valid_name: fn(&str) -> bool,
) -> Option<usize> {
    let level = stack.len();
    if level > 2 {
        return None;
    }
    let name = bytes.get(range.0..range.1).and_then(decode_string);
    if level == 1 {
        let name = name?;
        return TOP_KEYS.iter().position(|k| *k == name);
    }
    let parent = match stack.first() {
        Some(Frame::Object { tag, .. }) => *tag,
        _ => None,
    };
    match parent {
        Some(PROPOSAL_INDEX) => {
            let name = name?;
            PROPOSAL_KEYS.iter().position(|k| *k == name)
        }
        Some(INVENTORY_INDEX) => {
            if !name.as_deref().is_some_and(valid_name) {
                scan.names_ok = false;
            }
            None
        }
        _ => None,
    }
}
fn step_punctuation(
    bytes: &[u8],
    stack: &mut Vec<Frame>,
    scan: &mut Scan,
    valid_name: fn(&str) -> bool,
    pos: usize,
    byte: u8,
) -> Step {
    let key_expected = matches!(
        stack.last(),
        Some(Frame::Object {
            next: Next::First | Next::Key,
            ..
        })
    );
    if key_expected && byte == b'"' {
        let end = string_end(bytes, pos)?;
        let tag = key_tag(bytes, stack, (pos, end), scan, valid_name);
        if let Some(Frame::Object {
            key,
            next,
            tag: current,
            ..
        }) = stack.last_mut()
        {
            *key = Some((pos, end));
            *current = tag;
            *next = Next::Colon;
        }
        return Some((end, None));
    }
    match stack.last_mut()? {
        Frame::Object { next, .. } => match (*next, byte) {
            (Next::First | Next::CommaOrEnd, b'}') => {
                Some((pos + 1, Some((close(stack)?, pos + 1))))
            }
            (Next::Colon, b':') => {
                *next = Next::Value;
                Some((pos + 1, None))
            }
            (Next::CommaOrEnd, b',') => {
                *next = Next::Key;
                Some((pos + 1, None))
            }
            _ => None,
        },
        Frame::Array { next, .. } => match (*next, byte) {
            (Next::CommaOrEnd, b',') => {
                *next = Next::Value;
                Some((pos + 1, None))
            }
            (Next::CommaOrEnd, b']') => Some((pos + 1, Some((close(stack)?, pos + 1)))),
            _ => None,
        },
    }
}
fn close(stack: &mut Vec<Frame>) -> Option<usize> {
    match stack.pop()? {
        Frame::Object { begin, .. } | Frame::Array { begin, .. } => Some(begin),
    }
}
/// A value finished inside the frame on top of the stack.
fn record(stack: &mut [Frame], scan: &mut Scan, start: usize, end: usize) -> Option<()> {
    let level = stack.len();
    match stack.last_mut()? {
        Frame::Array { next, .. } => *next = Next::CommaOrEnd,
        Frame::Object { next, tag, .. } => {
            *next = Next::CommaOrEnd;
            if let Some(index) = *tag {
                match level {
                    1 => scan.top.get_mut(index)?.note(start, end),
                    2 => scan.proposal.get_mut(index)?.note(start, end),
                    _ => {}
                }
            }
        }
    }
    Some(())
}
