use crate::{Error, Result, ownership_codec as codec};
use hf_domain::{
    ownership::{Digest, Id},
    process::ProcessExit,
    task_lifecycle::*,
};
pub(crate) const SCHEMA: &str = "CREATE TABLE lifecycle_records(task TEXT PRIMARY KEY NOT NULL,operation TEXT UNIQUE NOT NULL REFERENCES ownership_records(operation),attempt TEXT UNIQUE NOT NULL,generation TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>=0),cancel_seq INTEGER,cancel_reason TEXT,cancel_at TEXT,observation TEXT NOT NULL); CREATE TABLE lifecycle_acceptance(task TEXT PRIMARY KEY NOT NULL REFERENCES lifecycle_records(task),endpoint TEXT NOT NULL CHECK(typeof(endpoint)='text' AND length(CAST(endpoint AS BLOB)) BETWEEN 1 AND 1024),source_digest TEXT NOT NULL CHECK(typeof(source_digest)='text' AND length(source_digest)=64 AND source_digest NOT GLOB '*[^0-9a-f]*')); CREATE TABLE lifecycle_intents(task TEXT PRIMARY KEY NOT NULL REFERENCES lifecycle_records(task),preparation_digest TEXT NOT NULL CHECK(typeof(preparation_digest)='text' AND length(preparation_digest)=64 AND preparation_digest NOT GLOB '*[^0-9a-f]*'),metadata BLOB NOT NULL CHECK(typeof(metadata)='blob' AND length(metadata)<=16384),metadata_digest TEXT NOT NULL CHECK(typeof(metadata_digest)='text' AND length(metadata_digest)=64 AND metadata_digest NOT GLOB '*[^0-9a-f]*'),prompt BLOB NOT NULL CHECK(typeof(prompt)='blob' AND length(prompt)<=4194304),prompt_digest TEXT NOT NULL CHECK(typeof(prompt_digest)='text' AND length(prompt_digest)=64 AND prompt_digest NOT GLOB '*[^0-9a-f]*'));";
pub(crate) fn encode(o: &ExecutionObservation) -> String {
    let result = o.result.as_ref().map_or_else(
        || "-".to_owned(),
        |r| {
            format!(
                "{}:{}:{}:{}",
                r.turn.as_str(),
                r.digest.as_str(),
                r.source_end,
                u8::from(r.success)
            )
        },
    );
    let exit = match o.exit {
        ProcessExit::Unobserved => "u".to_owned(),
        ProcessExit::Code(c) => format!("c{c}"),
        ProcessExit::Signal(s) => format!("s{s}"),
    };
    let mut text = format!(
        "{result}\n{exit}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        u8::from(o.drained),
        u8::from(o.input_closed),
        u8::from(o.capture_known_loss),
        o.stdout_end,
        o.stderr_end,
        u8::from(o.observation_gap),
        u8::from(o.event_projection_truncated),
        match o.obligations {
            Completeness::Unknown => 0,
            Completeness::Partial => 1,
            Completeness::Complete => 2,
        },
        u8::from(o.requirement == Requirement::Required),
        o.descendant_scan.as_ref().map_or("-", Id::as_str)
    );
    text.push_str(&format!(
        "\n{}\n{}",
        o.normalized_event_bytes, o.omitted_events
    ));
    text.push('\n');
    match &o.obligation_witness {
        None => text.push('-'),
        Some(w) => {
            text.push_str(&format!(
                "{}:{}:{}:{}",
                w.turn.as_str(),
                w.record_digest.as_str(),
                w.source_start,
                w.source_end
            ));
            for item in &w.observed_items {
                text.push_str(&format!(";{}:{}", item.id.as_str(), u8::from(item.failed)));
            }
        }
    }
    for p in &o.descendants {
        text.push('\n');
        text.push_str(&codec::process(p));
    }
    text
}
fn number(v: Option<&str>) -> Result<u64> {
    v.and_then(|v| v.parse().ok())
        .ok_or(Error::InvalidOwnership)
}
fn flag(v: Option<&str>) -> Result<bool> {
    match v {
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        _ => Err(Error::InvalidOwnership),
    }
}
pub(crate) fn decode(text: &str) -> Result<ExecutionObservation> {
    if text.len() > 16_384 {
        return Err(Error::LimitExceeded);
    }
    let mut fields = text.split('\n');
    let result = match fields.next() {
        Some("-") => None,
        Some(s) => Some(result(s)?),
        None => return Err(Error::InvalidOwnership),
    };
    let exit = match fields.next() {
        Some("u") => ProcessExit::Unobserved,
        Some(s) if s.starts_with('c') => {
            ProcessExit::Code(s[1..].parse().map_err(|_| Error::InvalidOwnership)?)
        }
        Some(s) if s.starts_with('s') => {
            ProcessExit::Signal(s[1..].parse().map_err(|_| Error::InvalidOwnership)?)
        }
        _ => return Err(Error::InvalidOwnership),
    };
    let drained = flag(fields.next())?;
    let input_closed = flag(fields.next())?;
    let capture_known_loss = flag(fields.next())?;
    let stdout_end = number(fields.next())?;
    let stderr_end = number(fields.next())?;
    let observation_gap = flag(fields.next())?;
    let event_projection_truncated = flag(fields.next())?;
    let obligations = match fields.next() {
        Some("0") => Completeness::Unknown,
        Some("1") => Completeness::Partial,
        Some("2") => Completeness::Complete,
        _ => return Err(Error::InvalidOwnership),
    };
    let requirement = if flag(fields.next())? {
        Requirement::Required
    } else {
        Requirement::Incidental
    };
    let descendant_scan = match fields.next() {
        Some("-") => None,
        Some(s) => Some(codec::id(s.to_owned())?),
        None => return Err(Error::InvalidOwnership),
    };
    let normalized_event_bytes = number(fields.next())?;
    let omitted_events = number(fields.next())?;
    let obligation_witness = match fields.next() {
        Some("-") => None,
        Some(text) => Some(obligation(text)?),
        None => return Err(Error::InvalidOwnership),
    };
    let mut descendants = Vec::new();
    for s in fields {
        if descendants.len() >= 64 {
            return Err(Error::LimitExceeded);
        }
        descendants.push(codec::decode_process(s)?);
    }
    Ok(ExecutionObservation {
        normalized_event_bytes,
        omitted_events,
        obligation_witness,
        result,
        exit,
        drained,
        input_closed,
        capture_known_loss,
        stdout_end,
        stderr_end,
        observation_gap,
        event_projection_truncated,
        obligations,
        requirement,
        descendant_scan,
        descendants,
    })
}
fn result(text: &str) -> Result<ResultWitness> {
    let mut f = text.split(':');
    let turn = codec::id(f.next().ok_or(Error::InvalidOwnership)?.to_owned())?;
    let digest = Digest::new(f.next().ok_or(Error::InvalidOwnership)?.to_owned())
        .map_err(|_| Error::InvalidOwnership)?;
    let source_end = number(f.next())?;
    let success = flag(f.next())?;
    if f.next().is_some() {
        return Err(Error::InvalidOwnership);
    }
    Ok(ResultWitness {
        turn,
        digest,
        source_end,
        success,
    })
}

fn obligation(text: &str) -> Result<ObligationWitness> {
    let mut parts = text.split(';');
    let mut f = parts.next().ok_or(Error::InvalidOwnership)?.split(':');
    let turn = codec::id(f.next().ok_or(Error::InvalidOwnership)?.to_owned())?;
    let record_digest = Digest::new(f.next().ok_or(Error::InvalidOwnership)?.to_owned())
        .map_err(|_| Error::InvalidOwnership)?;
    let source_start = number(f.next())?;
    let source_end = number(f.next())?;
    if f.next().is_some() {
        return Err(Error::InvalidOwnership);
    }
    let mut observed_items = Vec::new();
    for item in parts {
        if observed_items.len() >= 64 {
            return Err(Error::LimitExceeded);
        }
        let mut f = item.split(':');
        let id = codec::id(f.next().ok_or(Error::InvalidOwnership)?.to_owned())?;
        let failed = flag(f.next())?;
        if f.next().is_some() {
            return Err(Error::InvalidOwnership);
        }
        observed_items.push(ObligationItem { id, failed });
    }
    Ok(ObligationWitness {
        turn,
        record_digest,
        source_start,
        source_end,
        observed_items,
    })
}
