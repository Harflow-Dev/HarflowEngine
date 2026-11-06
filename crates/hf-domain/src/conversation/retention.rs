use super::{Error, Ledger, MAX_PROSE, MAX_RECEIPTS, Message, MessageState, Result};
impl Ledger {
    pub fn outstanding(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| {
                m.state == MessageState::Queued
                    || m.state == MessageState::Claimed
                    || m.state == MessageState::Unknown
            })
            .count()
    }
    pub fn retained_prose(&self) -> usize {
        self.messages.iter().map(|m| m.intent.text.len()).sum()
    }
    pub fn settled_receipts(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| m.state == MessageState::Consumed || m.state == MessageState::Cancelled)
            .map(|m| 1 + usize::from(m.cancellation.is_some()))
            .sum()
    }
    pub(super) fn make_space(&mut self, prose: usize, receipts: usize) -> Result<()> {
        let mut retired = false;
        while self
            .retained_prose()
            .checked_add(prose)
            .ok_or(Error::Capacity)?
            > MAX_PROSE
            || self.settled_receipts() + receipts > MAX_RECEIPTS
        {
            let index = self
                .messages
                .iter()
                .position(|m| self.eligible(m))
                .ok_or(Error::Capacity)?;
            if !retired {
                self.ledger_epoch = self.ledger_epoch.checked_add(1).ok_or(Error::Capacity)?;
                retired = true;
            }
            let message = self.messages.remove(index);
            self.history_floor = self
                .history_floor
                .max(message.sequence.checked_add(1).ok_or(Error::Capacity)?);
            self.retired_messages = self
                .retired_messages
                .checked_add(1)
                .ok_or(Error::Capacity)?;
            self.history_gaps = true;
        }
        Ok(())
    }
    fn eligible(&self, message: &Message) -> bool {
        match message.state {
            MessageState::Cancelled => true,
            MessageState::Consumed => message
                .claim
                .as_ref()
                .is_some_and(|claim| claim.generation < self.generation || self.boundary.is_some()),
            _ => false,
        }
    }
}
