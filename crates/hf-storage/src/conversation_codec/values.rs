use super::{Reader, Writer};
use crate::{Error, Result};
use hf_domain::{conversation::*, ownership::*, process::ProcessIdentity};
pub(super) fn read_id(r: &mut Reader<'_>) -> Result<Id> {
    let text = r.text(32)?;
    let id = Id::new(text.clone()).map_err(|_| Error::Incompatible)?;
    if id.as_str() != text {
        return Err(Error::Incompatible);
    }
    Ok(id)
}
pub(super) fn read_digest(r: &mut Reader<'_>) -> Result<Digest> {
    let text = r.text(64)?;
    let d = Digest::new(text.clone()).map_err(|_| Error::Incompatible)?;
    if d.as_str() != text {
        return Err(Error::Incompatible);
    }
    Ok(d)
}
fn process(w: &mut Writer, p: &ProcessIdentity) {
    w.number(u64::from(p.pid()));
    w.number(p.start_seconds());
    w.number(p.start_microseconds());
    w.text(p.boot_uuid());
}
fn read_process(r: &mut Reader<'_>) -> Result<ProcessIdentity> {
    ProcessIdentity::new(
        u32::try_from(r.number()?).map_err(|_| Error::Incompatible)?,
        r.number()?,
        r.number()?,
        r.text(36)?,
    )
    .map_err(|_| Error::Incompatible)
}
pub(super) fn enrollment(w: &mut Writer, e: &Enrollment) {
    for id in [
        &e.conversation,
        &e.head_task,
        &e.source.operation,
        &e.source.session,
        &e.source.reservation,
        &e.source.attempt,
        &e.source.generation,
    ] {
        w.text(id.as_str());
    }
    w.text(e.source.digest.as_str());
    w.bytes(&e.source.payload);
    for id in [
        &e.fence.operation,
        &e.fence.generation,
        &e.fence.attempt,
        &e.fence.owner.owner,
        &e.fence.owner.nonce,
    ] {
        w.text(id.as_str());
    }
    process(w, &e.fence.owner.process);
    w.number(e.fence.revision);
    process(w, &e.child);
    w.text(e.context.as_str());
    w.text(e.profile.as_str());
    w.text(e.turn.as_str());
    for n in [e.generation, e.ordinary_turns, e.node_attempts] {
        w.number(n);
    }
}
pub(super) fn read_enrollment(r: &mut Reader<'_>) -> Result<Enrollment> {
    let conversation = read_id(r)?;
    let head_task = read_id(r)?;
    let source = PreparedRequest {
        operation: read_id(r)?,
        session: read_id(r)?,
        reservation: read_id(r)?,
        attempt: read_id(r)?,
        generation: read_id(r)?,
        digest: read_digest(r)?,
        payload: r.bytes(16384)?,
    };
    let operation = read_id(r)?;
    let generation = read_id(r)?;
    let attempt = read_id(r)?;
    let owner = OwnerClaim {
        owner: read_id(r)?,
        nonce: read_id(r)?,
        process: read_process(r)?,
    };
    let fence = Fence {
        operation,
        generation,
        attempt,
        owner,
        revision: r.number()?,
    };
    Ok(Enrollment {
        conversation,
        head_task,
        source,
        fence,
        child: read_process(r)?,
        context: read_id(r)?,
        profile: read_digest(r)?,
        turn: read_id(r)?,
        generation: r.number()?,
        ordinary_turns: r.number()?,
        node_attempts: r.number()?,
    })
}
pub(super) fn witness(w: &mut Writer, v: &Witness) {
    for id in [&v.conversation, &v.head_task, &v.source_operation] {
        w.text(id.as_str());
    }
    w.number(v.generation);
    w.text(v.turn.as_str());
    w.number(v.ledger_epoch);
}
pub(super) fn read_witness(r: &mut Reader<'_>) -> Result<Witness> {
    Ok(Witness {
        conversation: read_id(r)?,
        head_task: read_id(r)?,
        source_operation: read_id(r)?,
        generation: r.number()?,
        turn: read_id(r)?,
        ledger_epoch: r.number()?,
    })
}
pub(super) fn claim(w: &mut Writer, c: &Claim) {
    w.text(c.intent.request.as_str());
    witness(w, &c.intent.expected);
    w.text(c.intent.destination.as_str());
    for id in [&c.message, &c.source, &c.context] {
        w.text(id.as_str());
    }
    w.number(c.generation);
    w.text(c.turn.as_str());
}
pub(super) fn read_claim(r: &mut Reader<'_>) -> Result<Claim> {
    let intent = ClaimIntent {
        request: read_id(r)?,
        expected: read_witness(r)?,
        destination: read_id(r)?,
    };
    Ok(Claim {
        intent,
        message: read_id(r)?,
        source: read_id(r)?,
        context: read_id(r)?,
        generation: r.number()?,
        turn: read_id(r)?,
    })
}
