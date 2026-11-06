use crate::{Error, Result};
use hf_domain::conversation::*;
mod values;
use values::*;
pub(crate) struct Writer(pub Vec<u8>);
impl Writer {
    fn number(&mut self, n: u64) {
        self.0.extend(n.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.number(b.len() as u64);
        self.0.extend(b);
    }
    fn text(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn boolean(&mut self, b: bool) {
        self.0.push(u8::from(b));
    }
}
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl Reader<'_> {
    fn take(&mut self, size: usize) -> Result<&[u8]> {
        let end = self
            .offset
            .checked_add(size)
            .filter(|v| *v <= self.bytes.len())
            .ok_or(Error::Incompatible)?;
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }
    fn number(&mut self) -> Result<u64> {
        let mut b = [0; 8];
        b.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(b))
    }
    fn bytes(&mut self, max: usize) -> Result<Vec<u8>> {
        let length = usize::try_from(self.number()?).map_err(|_| Error::Incompatible)?;
        if length > max {
            return Err(Error::LimitExceeded);
        }
        Ok(self.take(length)?.to_vec())
    }
    fn text(&mut self, max: usize) -> Result<String> {
        String::from_utf8(self.bytes(max)?).map_err(|_| Error::Incompatible)
    }
    fn boolean(&mut self) -> Result<bool> {
        match self.take(1)? {
            [0] => Ok(false),
            [1] => Ok(true),
            _ => Err(Error::Incompatible),
        }
    }
}
pub(crate) fn encode(root: &str, value: &Ledger) -> Result<Vec<u8>> {
    value.validate().map_err(Error::Conversation)?;
    let mut w = Writer(b"HFCQ1".to_vec());
    w.text(root);
    enrollment(&mut w, &value.enrollment);
    for n in [
        value.generation,
        value.ledger_epoch,
        value.next_sequence,
        value.cumulative_issuances,
        value.unavailable_usage,
        value.history_floor,
        value.retired_messages,
    ] {
        w.number(n);
    }
    w.text(value.turn.as_str());
    for b in [value.closed, value.runtime_fault, value.history_gaps] {
        w.boolean(b);
    }
    w.boolean(value.boundary.is_some());
    if let Some(b) = &value.boundary {
        witness(&mut w, &b.expected);
        w.text(b.context.as_str());
        w.text(b.profile.as_str());
        w.text(b.evidence.as_str());
    }
    w.number(value.messages.len() as u64);
    for m in &value.messages {
        message(&mut w, m);
    }
    if w.0.len() > MAX_FRAME {
        return Err(Error::LimitExceeded);
    }
    Ok(w.0)
}
pub(crate) fn decode(root: &str, bytes: &[u8]) -> Result<Ledger> {
    if bytes.len() > MAX_FRAME {
        return Err(Error::LimitExceeded);
    }
    let mut r = Reader { bytes, offset: 0 };
    if r.take(5)? != b"HFCQ1" || r.text(32)? != root {
        return Err(Error::Incompatible);
    }
    let enrollment = read_enrollment(&mut r)?;
    let generation = r.number()?;
    let ledger_epoch = r.number()?;
    let next_sequence = r.number()?;
    let cumulative_issuances = r.number()?;
    let unavailable_usage = r.number()?;
    let history_floor = r.number()?;
    let retired_messages = r.number()?;
    let turn = read_id(&mut r)?;
    let closed = r.boolean()?;
    let runtime_fault = r.boolean()?;
    let history_gaps = r.boolean()?;
    let boundary = if r.boolean()? {
        Some(BoundaryEvidence {
            expected: read_witness(&mut r)?,
            context: read_id(&mut r)?,
            profile: read_digest(&mut r)?,
            evidence: read_id(&mut r)?,
        })
    } else {
        None
    };
    let count = r.number()?;
    if count > (MAX_OUTSTANDING + MAX_RECEIPTS) as u64 {
        return Err(Error::LimitExceeded);
    }
    let mut messages = Vec::new();
    for _ in 0..count {
        messages.push(read_message(&mut r)?);
    }
    if r.offset != bytes.len() {
        return Err(Error::Incompatible);
    }
    let value = Ledger {
        enrollment,
        generation,
        turn,
        ledger_epoch,
        boundary,
        closed,
        runtime_fault,
        next_sequence,
        cumulative_issuances,
        unavailable_usage,
        history_floor,
        retired_messages,
        history_gaps,
        messages,
    };
    value.validate().map_err(Error::Conversation)?;
    Ok(value)
}
fn message(w: &mut Writer, m: &Message) {
    w.text(m.intent.request.as_str());
    witness(w, &m.intent.expected);
    w.text(&m.intent.text);
    w.number(m.sequence);
    w.0.push(match m.state {
        MessageState::Queued => 0,
        MessageState::Claimed => 1,
        MessageState::Consumed => 2,
        MessageState::Unknown => 3,
        MessageState::Cancelled => 4,
    });
    w.boolean(m.acknowledged);
    w.boolean(m.claim.is_some());
    if let Some(c) = &m.claim {
        claim(w, c);
    }
    w.boolean(m.receipt.is_some());
    if let Some(r) = &m.receipt {
        claim(w, &r.claim);
        w.text(r.evidence.as_str());
    }
    w.boolean(m.cancellation.is_some());
    if let Some(c) = &m.cancellation {
        w.text(c.request.as_str());
        w.text(c.message.as_str());
        witness(w, &c.expected);
    }
}
fn read_message(r: &mut Reader<'_>) -> Result<Message> {
    let intent = SendIntent {
        request: read_id(r)?,
        expected: read_witness(r)?,
        text: r.text(MAX_TEXT)?,
    };
    let sequence = r.number()?;
    let state = match r.take(1)? {
        [0] => MessageState::Queued,
        [1] => MessageState::Claimed,
        [2] => MessageState::Consumed,
        [3] => MessageState::Unknown,
        [4] => MessageState::Cancelled,
        _ => return Err(Error::Incompatible),
    };
    let acknowledged = r.boolean()?;
    let claim = if r.boolean()? {
        Some(read_claim(r)?)
    } else {
        None
    };
    let receipt = if r.boolean()? {
        Some(Receipt {
            claim: read_claim(r)?,
            evidence: read_id(r)?,
        })
    } else {
        None
    };
    let cancellation = if r.boolean()? {
        Some(CancelIntent {
            request: read_id(r)?,
            message: read_id(r)?,
            expected: read_witness(r)?,
        })
    } else {
        None
    };
    Ok(Message {
        intent,
        sequence,
        state,
        claim,
        acknowledged,
        receipt,
        cancellation,
    })
}
