use super::{
    Error, Id, Ledger, MAX_OUTSTANDING, MAX_PROSE, MAX_RECEIPTS, MAX_TEXT, Message, MessageState,
    Result, Witness,
};
impl Ledger {
    pub fn validate(&self) -> Result<()> {
        let e = &self.enrollment;
        if e.source.operation != e.fence.operation
            || e.source.generation != e.fence.generation
            || e.source.attempt != e.fence.attempt
            || e.source.payload.len() > 16384
            || e.fence.revision < 3
            || self.generation < e.generation
            || self.generation - e.generation != self.cumulative_issuances
            || self.unavailable_usage != self.cumulative_issuances
            || self.next_sequence == 0
            || self.history_floor == 0
            || self.history_floor > self.next_sequence
            || self.retained_prose() > MAX_PROSE
            || self.outstanding() > MAX_OUTSTANDING
            || self.settled_receipts() > MAX_RECEIPTS
            || self.messages.len() > MAX_OUTSTANDING + MAX_RECEIPTS
            || self
                .retired_messages
                .checked_add(self.messages.len() as u64)
                != Some(self.next_sequence - 1)
            || self.history_gaps != (self.retired_messages != 0)
        {
            return Err(Error::Invalid);
        }
        if let Some(b) = &self.boundary
            && ((b.expected.conversation != e.conversation
                || b.expected.head_task != e.head_task
                || b.expected.source_operation != e.source.operation
                || b.expected.generation != self.generation
                || b.expected.turn != self.turn
                || b.expected.ledger_epoch > self.ledger_epoch)
                || b.context != e.context
                || b.profile != e.profile)
        {
            return Err(Error::Invalid);
        }
        let mut ids: Vec<&Id> = Vec::new();
        let mut sequence = 0;
        let mut active = 0;
        for message in &self.messages {
            if message.sequence <= sequence
                || message.sequence >= self.next_sequence
                || message.intent.text.is_empty()
                || message.intent.text.len() > MAX_TEXT
                || !self.historical(&message.intent.expected)
            {
                return Err(Error::Invalid);
            }
            sequence = message.sequence;
            ids.push(&message.intent.request);
            self.validate_message(message)?;
            if let Some(c) = &message.claim {
                ids.push(&c.intent.request);
            }
            if let Some(c) = &message.cancellation {
                ids.push(&c.request);
            }
            active += usize::from(
                message.state == MessageState::Claimed || message.state == MessageState::Unknown,
            );
        }
        if active > 1
            || (self.boundary.is_some() && active != 0)
            || (self.closed
                && (self.outstanding() != 0 || self.boundary.is_none() || self.runtime_fault))
        {
            return Err(Error::Invalid);
        }
        for (index, id) in ids.iter().enumerate() {
            if ids[..index].contains(id) {
                return Err(Error::Invalid);
            }
        }
        Ok(())
    }
    fn historical(&self, w: &Witness) -> bool {
        w.conversation == self.enrollment.conversation
            && w.head_task == self.enrollment.head_task
            && w.source_operation == self.enrollment.source.operation
            && w.ledger_epoch <= self.ledger_epoch
            && w.generation >= self.enrollment.generation
            && w.generation <= self.generation
            && (w.generation != self.enrollment.generation || w.turn == self.enrollment.turn)
            && (w.generation != self.generation || w.turn == self.turn)
    }
    fn validate_message(&self, m: &Message) -> Result<()> {
        match m.state {
            MessageState::Queued => {
                if m.claim.is_some()
                    || m.receipt.is_some()
                    || m.cancellation.is_some()
                    || m.acknowledged
                {
                    return Err(Error::Invalid);
                }
            }
            MessageState::Cancelled => {
                if m.claim.is_some()
                    || m.receipt.is_some()
                    || m.cancellation.is_none()
                    || m.acknowledged
                {
                    return Err(Error::Invalid);
                }
            }
            MessageState::Claimed | MessageState::Unknown => {
                if m.claim.is_none() || m.receipt.is_some() || m.cancellation.is_some() {
                    return Err(Error::Invalid);
                }
            }
            MessageState::Consumed => {
                if m.claim.is_none() || m.receipt.is_none() || m.cancellation.is_some() {
                    return Err(Error::Invalid);
                }
            }
        }
        if let Some(c) = &m.claim
            && (c.message != m.intent.request
                || c.context != self.enrollment.context
                || c.source != self.enrollment.source.operation
                || !self.historical(&c.intent.expected)
                || c.intent.expected.generation.checked_add(1) != Some(c.generation)
                || c.intent.destination != c.turn
                || c.intent.expected.turn == c.turn
                || c.generation > self.generation
                || m.receipt.as_ref().is_some_and(|r| r.claim != *c))
        {
            return Err(Error::Invalid);
        }
        if let Some(c) = &m.cancellation
            && (c.message != m.intent.request || !self.historical(&c.expected))
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
