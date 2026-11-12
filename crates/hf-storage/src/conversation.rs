use crate::{
    Error, Result, Store, connection, conversation_data as data, ownership_schema,
    write_tx::{Publish, Stage},
};
use hf_application::conversation::{Command, ConversationStorePort};
use hf_domain::{conversation::*, ownership::Id};
use rusqlite::{Transaction, params};
use std::time::Instant;
/// One-shot offline fixture token, created only after a new claim commit.
#[derive(Debug, PartialEq, Eq)]
pub struct FreshGrant {
    claim: Claim,
}
impl FreshGrant {
    pub fn claim(&self) -> &Claim {
        &self.claim
    }
    pub fn into_claim(self) -> Claim {
        self.claim
    }
}
fn transaction<T>(
    store: &Store,
    deadline: Instant,
    action: impl FnOnce(&Transaction<'_>) -> Result<T>,
) -> Result<T> {
    let result = store.write(
        deadline,
        Publish::Nothing,
        Some(Stage::CONVERSATION),
        |tx| {
            ownership_schema::check(tx, &store.identity.generation)?;
            action(tx)
        },
    )?;
    connection::check_deadline(deadline).map_err(|_| Error::CommitOutcomeUnknown)?;
    Ok(result)
}
impl Store {
    pub fn conversation_enroll(&self, e: &Enrollment, deadline: Instant) -> Result<Ledger> {
        transaction(self, deadline, |tx| {
            let expected = data::binding(self, e)?;
            let source = data::blob(self, tx, &data::key("source", &e.source.operation))?;
            let context = data::blob(self, tx, &data::key("context", &e.context))?;
            let ledger = data::blob(self, tx, &data::key("ledger", &e.conversation))?;
            if source.is_some() || context.is_some() || ledger.is_some() {
                if source.as_ref().is_some_and(|v| v != &expected)
                    || context.as_ref().is_some_and(|v| v != &expected)
                {
                    return Err(Error::OwnershipConflict);
                }
                if source.is_none() || context.is_none() || ledger.is_none() {
                    return Err(Error::Incompatible);
                }
                let found = data::load(self, tx, &e.conversation)?;
                if found.enrollment != *e {
                    return Err(Error::OwnershipConflict);
                }
                return Ok(found);
            }
            data::source(self, tx, e, true)?;
            let ledger = Ledger::new(e.clone()).map_err(Error::Conversation)?;
            data::persist(self, tx, &ledger, true)?;
            for key in [
                data::key("source", &e.source.operation),
                data::key("context", &e.context),
            ] {
                tx.execute(
                    "INSERT INTO fixture_records(key,value) VALUES(?1,?2)",
                    params![key, expected],
                )?;
            }
            Ok(ledger)
        })
    }
    pub fn conversation_observe(&self, id: &Id, deadline: Instant) -> Result<Ledger> {
        let mut connection = self.admit(deadline)?;
        let tx = connection.transaction()?;
        ownership_schema::check(&tx, &self.identity.generation)?;
        let ledger = data::load(self, &tx, id)?;
        self.binding.check()?;
        connection::check_deadline(deadline)?;
        Ok(ledger)
    }
    pub fn conversation_apply(
        &self,
        id: &Id,
        command: Command,
        deadline: Instant,
    ) -> Result<ClaimOutcome<FreshGrant>> {
        let (ledger, fresh) = transaction(self, deadline, |tx| {
            let mut ledger = data::load(self, tx, id)?;
            let admission = matches!(&command, Command::Send(_));
            let active = match &command {
                Command::Send(i) => !ledger.send_replay(i).map_err(Error::Conversation)?,
                Command::Claim(i) => ledger
                    .claim_replay(i)
                    .map_err(Error::Conversation)?
                    .is_none(),
                _ => false,
            };
            if active {
                data::source(self, tx, &ledger.enrollment, true)?;
            }
            let fresh = transition(&mut ledger, command).map_err(Error::Conversation)?;
            data::persist(self, tx, &ledger, admission && active)?;
            Ok((ledger, fresh))
        })?;
        Ok(ClaimOutcome {
            ledger,
            grant: fresh.map(|claim| FreshGrant { claim }),
        })
    }
}
fn transition(
    ledger: &mut Ledger,
    command: Command,
) -> hf_domain::conversation::Result<Option<Claim>> {
    match command {
        Command::Send(i) => ledger.send(i)?,
        Command::Cancel(i) => ledger.cancel(i)?,
        Command::Claim(i) => return ledger.claim(i),
        Command::Boundary(e) => ledger.record_boundary(e)?,
        Command::Receipt(r) => ledger.receipt(r)?,
        Command::Acknowledge(c) => ledger.acknowledge(&c)?,
        Command::Unknown(c) => ledger.unknown(&c)?,
        Command::Close(w) => ledger.close(&w)?,
        Command::RuntimeFault { generation, turn } => ledger.fault(generation, &turn)?,
    }
    Ok(None)
}
impl ConversationStorePort for Store {
    type Error = Error;
    type Grant = FreshGrant;
    type Deadline = Instant;
    fn enroll(&self, e: &Enrollment, d: Instant) -> Result<Ledger> {
        self.conversation_enroll(e, d)
    }
    fn observe(&self, id: &Id, d: Instant) -> Result<Ledger> {
        self.conversation_observe(id, d)
    }
    fn apply(&self, id: &Id, c: Command, d: Instant) -> Result<ClaimOutcome<FreshGrant>> {
        self.conversation_apply(id, c, d)
    }
}
