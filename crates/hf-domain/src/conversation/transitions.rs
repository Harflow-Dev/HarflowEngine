use super::{
    CancelIntent, Claim, ClaimIntent, Enrollment, Error, Id, Ledger, MAX_OUTSTANDING, MAX_TEXT,
    Message, MessageState, Result, SendIntent, Witness,
};
impl Ledger {
    pub fn new(enrollment: Enrollment) -> Result<Self> {
        let value = Self {
            generation: enrollment.generation,
            turn: enrollment.turn.clone(),
            enrollment,
            ledger_epoch: 0,
            boundary: None,
            closed: false,
            runtime_fault: false,
            next_sequence: 1,
            cumulative_issuances: 0,
            unavailable_usage: 0,
            history_floor: 1,
            retired_messages: 0,
            history_gaps: false,
            messages: Vec::new(),
        };
        value.validate()?;
        Ok(value)
    }
    pub fn witness(&self) -> Witness {
        Witness {
            conversation: self.enrollment.conversation.clone(),
            head_task: self.enrollment.head_task.clone(),
            source_operation: self.enrollment.source.operation.clone(),
            generation: self.generation,
            turn: self.turn.clone(),
            ledger_epoch: self.ledger_epoch,
        }
    }
    pub fn send_replay(&self, intent: &SendIntent) -> Result<bool> {
        if let Some(m) = self
            .messages
            .iter()
            .find(|m| m.intent.request == intent.request)
        {
            return if m.intent == *intent {
                Ok(true)
            } else {
                Err(Error::Conflict)
            };
        }
        if self.control_id(&intent.request) {
            return Err(Error::Conflict);
        }
        Ok(false)
    }
    pub fn send(&mut self, intent: SendIntent) -> Result<()> {
        if self.send_replay(&intent)? {
            return Ok(());
        }
        self.expected(&intent.expected)?;
        self.open()?;
        if intent.text.is_empty() || intent.text.len() > MAX_TEXT {
            return Err(Error::Invalid);
        }
        if self.outstanding() >= MAX_OUTSTANDING {
            return Err(Error::Capacity);
        }
        let mut next = self.clone();
        next.make_space(intent.text.len(), 1)?;
        let sequence = next.next_sequence;
        next.next_sequence = sequence.checked_add(1).ok_or(Error::Capacity)?;
        next.messages.push(Message {
            intent,
            sequence,
            state: MessageState::Queued,
            claim: None,
            acknowledged: false,
            receipt: None,
            cancellation: None,
        });
        *self = next;
        Ok(())
    }
    pub fn cancel_replay(&self, intent: &CancelIntent) -> Result<bool> {
        for message in &self.messages {
            if let Some(old) = &message.cancellation
                && old.request == intent.request
            {
                return if old == intent {
                    Ok(true)
                } else {
                    Err(Error::Conflict)
                };
            }
        }
        if self.any_id(&intent.request) {
            return Err(Error::Conflict);
        }
        Ok(false)
    }
    pub fn cancel(&mut self, intent: CancelIntent) -> Result<()> {
        if self.cancel_replay(&intent)? {
            return Ok(());
        }
        self.expected(&intent.expected)?;
        let mut next = self.clone();
        let index = next
            .messages
            .iter()
            .position(|m| m.intent.request == intent.message)
            .ok_or(Error::Missing)?;
        if next.messages[index].state != MessageState::Queued {
            return Err(Error::AlreadyClaimed);
        }
        // A cancellation receipt and its send become settled together.
        next.make_space(0, 2)?;
        let message = next
            .messages
            .iter_mut()
            .find(|m| m.intent.request == intent.message)
            .ok_or(Error::Missing)?;
        message.state = MessageState::Cancelled;
        message.cancellation = Some(intent);
        *self = next;
        Ok(())
    }
    pub fn claim_replay(&self, intent: &ClaimIntent) -> Result<Option<Claim>> {
        for message in &self.messages {
            if let Some(claim) = &message.claim
                && claim.intent.request == intent.request
            {
                return if claim.intent == *intent {
                    Ok(Some(claim.clone()))
                } else {
                    Err(Error::Conflict)
                };
            }
        }
        if self.any_id(&intent.request) {
            return Err(Error::Conflict);
        }
        Ok(None)
    }
    pub fn claim(&mut self, intent: ClaimIntent) -> Result<Option<Claim>> {
        if self.claim_replay(&intent)?.is_some() {
            return Ok(None);
        }
        self.expected(&intent.expected)?;
        self.open()?;
        if self.boundary.is_none()
            || self
                .messages
                .iter()
                .any(|m| m.state == MessageState::Claimed || m.state == MessageState::Unknown)
        {
            return Err(Error::Blocked);
        }
        let index = self
            .messages
            .iter()
            .position(|m| m.state == MessageState::Queued)
            .ok_or(Error::Missing)?;
        let generation = self.generation.checked_add(1).ok_or(Error::Capacity)?;
        if intent.destination == self.turn {
            return Err(Error::Conflict);
        }
        let turn = intent.destination.clone();
        let count = self
            .cumulative_issuances
            .checked_add(1)
            .ok_or(Error::Capacity)?;
        let claim = Claim {
            intent,
            message: self.messages[index].intent.request.clone(),
            source: self.enrollment.source.operation.clone(),
            context: self.enrollment.context.clone(),
            generation,
            turn: turn.clone(),
        };
        self.messages[index].claim = Some(claim.clone());
        self.messages[index].state = MessageState::Claimed;
        self.generation = generation;
        self.turn = turn;
        self.cumulative_issuances = count;
        self.unavailable_usage = count;
        self.boundary = None;
        Ok(Some(claim))
    }
    pub(super) fn expected(&self, expected: &Witness) -> Result<()> {
        if expected.ledger_epoch != self.ledger_epoch {
            return Err(Error::Expired);
        }
        if *expected != self.witness() {
            return Err(Error::Stale);
        }
        Ok(())
    }
    fn open(&self) -> Result<()> {
        if self.closed {
            Err(Error::Closed)
        } else if self.runtime_fault {
            Err(Error::Fault)
        } else {
            Ok(())
        }
    }
    fn control_id(&self, id: &Id) -> bool {
        self.messages.iter().any(|m| {
            m.cancellation.as_ref().is_some_and(|v| v.request == *id)
                || m.claim.as_ref().is_some_and(|v| v.intent.request == *id)
        })
    }
    fn any_id(&self, id: &Id) -> bool {
        self.control_id(id) || self.messages.iter().any(|m| m.intent.request == *id)
    }
}
