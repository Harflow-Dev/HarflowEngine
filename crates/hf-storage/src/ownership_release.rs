use crate::{Error, Result, ownership_codec as codec};
use hf_domain::{ownership::*, process::ProcessExit};
pub(crate) fn audit(snapshot: &Snapshot, proof: &ReleaseProof) -> Result<String> {
    if !codec::revision_valid(snapshot) {
        return Err(Error::InvalidOwnership);
    }
    let (evidence, facts) = match proof {
        ReleaseProof::Unclaimed { evidence }
            if snapshot.phase == Phase::Prepared
                && snapshot.cancelled
                && snapshot.owner.is_none()
                && snapshot.child.is_none() =>
        {
            (evidence, "unclaimed".to_owned())
        }
        ReleaseProof::NeverStarted {
            evidence,
            owner_dead: true,
        } if snapshot.phase == Phase::Claimed
            && snapshot.child.is_none()
            && snapshot.owner.is_some() =>
        {
            (evidence, "never-started:dead".to_owned())
        }
        ReleaseProof::Terminal {
            evidence,
            owner_dead: true,
            child,
            exit,
            drain_complete: true,
            containment_complete: true,
            capture,
        } if matches!(
            snapshot.phase,
            Phase::SpawnObserved | Phase::AcceptanceRecorded
        ) && snapshot.child.as_ref() == Some(child)
            && snapshot.owner.is_some() =>
        {
            let terminal = match exit {
                ProcessExit::Code(code) => format!("code:{code}"),
                ProcessExit::Signal(signal) if *signal > 0 => format!("signal:{signal}"),
                _ => return Err(Error::InvalidOwnership),
            };
            let capture = match capture {
                CaptureAudit::Unknown => "unknown",
                CaptureAudit::Lossless => "lossless",
                CaptureAudit::KnownLoss => "loss",
            };
            (
                evidence,
                format!("terminal:dead:{terminal}:drained:contained:{capture}"),
            )
        }
        _ => return Err(Error::InvalidOwnership),
    };
    let request = &snapshot.request;
    let owner = snapshot.owner.as_ref();
    Ok(format!(
        "1|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        evidence.as_str(),
        request.operation.as_str(),
        request.session.as_str(),
        request.reservation.as_str(),
        request.attempt.as_str(),
        request.generation.as_str(),
        codec::phase_number(snapshot.phase),
        snapshot.revision,
        u8::from(snapshot.cancelled),
        owner.map_or("-", |v| v.owner.as_str()),
        owner.map_or("-", |v| v.nonce.as_str()),
        owner.map_or_else(|| "-".to_owned(), |v| codec::process(&v.process)),
        snapshot
            .child
            .as_ref()
            .map_or_else(|| "-".to_owned(), codec::process),
        facts
    ))
}
pub(crate) fn validate(snapshot: &Snapshot, text: &str) -> Result<()> {
    let fields: Vec<&str> = text.split('|').collect();
    if fields.len() != 15 || fields[0] != "1" {
        return Err(Error::InvalidOwnership);
    }
    let evidence = codec::id(fields[1].to_owned())?;
    let mut prior = snapshot.clone();
    prior.phase = codec::phase(fields[7].parse().map_err(|_| Error::InvalidOwnership)?)?;
    prior.revision = snapshot
        .revision
        .checked_sub(1)
        .ok_or(Error::InvalidOwnership)?;
    let proof = match fields[14] {
        "unclaimed" => ReleaseProof::Unclaimed { evidence },
        "never-started:dead" => ReleaseProof::NeverStarted {
            evidence,
            owner_dead: true,
        },
        facts => terminal(&prior, evidence, facts)?,
    };
    if audit(&prior, &proof)? != text {
        return Err(Error::InvalidOwnership);
    }
    Ok(())
}
fn terminal(snapshot: &Snapshot, evidence: Id, text: &str) -> Result<ReleaseProof> {
    let fields: Vec<&str> = text.split(':').collect();
    if fields.len() != 7
        || fields[0] != "terminal"
        || fields[1] != "dead"
        || fields[4] != "drained"
        || fields[5] != "contained"
    {
        return Err(Error::InvalidOwnership);
    }
    let value: i32 = fields[3].parse().map_err(|_| Error::InvalidOwnership)?;
    let exit = match fields[2] {
        "code" => ProcessExit::Code(value),
        "signal" => ProcessExit::Signal(value),
        _ => return Err(Error::InvalidOwnership),
    };
    let capture = match fields[6] {
        "unknown" => CaptureAudit::Unknown,
        "lossless" => CaptureAudit::Lossless,
        "loss" => CaptureAudit::KnownLoss,
        _ => return Err(Error::InvalidOwnership),
    };
    Ok(ReleaseProof::Terminal {
        evidence,
        owner_dead: true,
        child: snapshot.child.clone().ok_or(Error::InvalidOwnership)?,
        exit,
        drain_complete: true,
        containment_complete: true,
        capture,
    })
}
