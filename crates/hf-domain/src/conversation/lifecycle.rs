use super::{
    BoundaryEvidence, Claim, Error, Ledger, Message, MessageState, Receipt, Result, Witness,
};
impl Ledger {
    pub fn acknowledge(&mut self, claim: &Claim) -> Result<()> {
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
            _ => Err(Error::Invalid),
        }
    }
    pub fn record_boundary(&mut self, evidence: BoundaryEvidence) -> Result<()> {
        self.expected(&evidence.expected)?;
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
        self.boundary = Some(evidence);
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
