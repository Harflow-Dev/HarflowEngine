use super::{
    BoundaryEvidence, Claim, Error, Id, Ledger, Message, MessageState, Receipt, Result, Witness,
};
impl Ledger {
    pub fn acknowledge(&mut self, claim: &Claim) -> Result<()> {
        if self.closed {
            return Err(Error::Closed);
        }
        let message = self.matching_claim(claim)?;
        if message.state != MessageState::Claimed
            && message.state != MessageState::Unknown
            && message.state != MessageState::Consumed
        {
            return Err(Error::Invalid);
        }
        message.acknowledged = true;
        Ok(())
    }
    pub fn receipt(&mut self, receipt: Receipt) -> Result<()> {
        let message = self.matching_claim(&receipt.claim)?;
        if let Some(old) = &message.receipt {
            return if old == &receipt {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        if message.state != MessageState::Claimed && message.state != MessageState::Unknown {
            return Err(Error::Invalid);
        }
        let mut next = self.clone();
        next.make_space(0, 1)?;
        let message = next.matching_claim(&receipt.claim)?;
        message.state = MessageState::Consumed;
        message.receipt = Some(receipt);
        *self = next;
        Ok(())
    }
    pub fn unknown(&mut self, claim: &Claim) -> Result<()> {
        let message = self.matching_claim(claim)?;
        match message.state {
            MessageState::Claimed | MessageState::Unknown => {
                message.state = MessageState::Unknown;
                Ok(())
            }
            MessageState::Consumed => Ok(()),
            _ => Err(Error::Invalid),
        }
    }
    pub fn record_boundary(&mut self, evidence: BoundaryEvidence) -> Result<()> {
        self.expected(&evidence.expected)?;
        if self.closed {
            return Err(Error::Closed);
        }
        if evidence.context != self.enrollment.context
            || evidence.profile != self.enrollment.profile
        {
            return Err(Error::Conflict);
        }
        if self.runtime_fault
            || self
                .messages
                .iter()
                .any(|m| m.state == MessageState::Claimed || m.state == MessageState::Unknown)
        {
            return Err(Error::Blocked);
        }
        if let Some(old) = &self.boundary {
            // First evidence wins and stays immutable, including its epoch.
            return if old.evidence == evidence.evidence {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        self.boundary = Some(evidence);
        Ok(())
    }
    /// Sticky and idempotent. The report must name the current generation and turn.
    pub fn fault(&mut self, generation: u64, turn: &Id) -> Result<()> {
        if self.generation != generation || self.turn != *turn {
            return Err(Error::Stale);
        }
        if self.closed {
            return Err(Error::Closed);
        }
        self.runtime_fault = true;
        Ok(())
    }
    pub fn close(&mut self, expected: &Witness) -> Result<()> {
        self.expected(expected)?;
        if self.runtime_fault || self.boundary.is_none() || self.outstanding() != 0 {
            return Err(Error::Blocked);
        }
        self.closed = true;
        Ok(())
    }
    fn matching_claim(&mut self, claim: &Claim) -> Result<&mut Message> {
        self.messages
            .iter_mut()
            .find(|m| m.claim.as_ref() == Some(claim))
            .ok_or(Error::Conflict)
    }
}
