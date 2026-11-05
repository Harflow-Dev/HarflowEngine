//! Original-owner persistence with independent bounded fixed-helper drainage.
use super::owned::OwnerChild;
use super::*;
use hf_application::owner::{OwnerPort, OwnerSession};
use hf_domain::{capture::CaptureBinding, ownership::Fence, process::ProcessExit};
use hf_platform::{
    fixture::RawCapture,
    process::{CaptureOrigin, ReaderFinal},
};
#[cfg(test)]
mod tests;
mod writer;

pub(super) fn complete(
    owned: &mut OwnerChild,
    prepared: &Prepared,
    raw: &mut RawCapture,
    session: &mut OwnerSession<Store>,
    gate: &FixtureGate,
    until: Instant,
) -> io::Result<Fence> {
    let origin = CaptureOrigin::from_spawn_observed(session.snapshot().ok_or_else(invalid)?)?;
    owned.reader = Some(
        owned
            .child
            .start_private_fixture_capture_reader(origin.clone(), until)?,
    );
    // FD handoff precedes every acceptance/capture storage mutation.
    if map(session.accept(until)).is_err() {
        session.poison();
        owned.reader.as_ref().ok_or_else(invalid)?.discard_capture();
    }
    let binding = if session.is_poisoned() {
        None
    } else {
        match accepted_binding(session, &origin, raw) {
            Ok(binding) => Some(binding),
            Err(_) => {
                session.poison();
                None
            }
        }
    };
    let accepted = binding.as_ref().map(|value| value.fence.clone());
    let mut reach = |stage| {
        gate.reach(
            stage,
            &prepared.invitation.operation,
            &prepared.invitation.nonce,
            until,
        )
    };
    let report = drive(owned, raw, session, binding.as_ref(), &mut reach, until)?;
    if !report.terminal
        || report.accounting.uncertain != (0, 0)
        || report.accounting.abandoned != (0, 0)
    {
        return Err(invalid());
    }
    accepted.ok_or_else(invalid)
}
use hf_storage::Store;
fn accepted_binding<P: OwnerPort>(
    session: &OwnerSession<P>,
    origin: &CaptureOrigin,
    raw: &RawCapture,
) -> io::Result<CaptureBinding> {
    let accepted = session
        .snapshot()
        .and_then(|s| s.fence())
        .ok_or_else(invalid)?;
    let mut expected = origin.fence.clone();
    expected.revision = expected.revision.checked_add(1).ok_or_else(invalid)?;
    if accepted != expected
        || session.snapshot().and_then(|s| s.child.as_ref()) != Some(&origin.child)
    {
        return Err(invalid());
    }
    let (stdout, stderr) = raw.identities();
    Ok(CaptureBinding {
        fence: accepted,
        child: origin.child.clone(),
        stdout,
        stderr,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CaptureAccounting {
    acknowledged: (u64, u64),
    abandoned: (u64, u64),
    /// Accepted bytes whose raw/index mutation was attempted but not acknowledged.
    uncertain: (u64, u64),
}
struct DrainReport {
    terminal: bool,
    accounting: CaptureAccounting,
}
/// Tests inject finite gates and a read-only reader view; production remains on owner main.
fn drive<P: OwnerPort>(
    owned: &mut OwnerChild,
    raw: &mut RawCapture,
    session: &mut OwnerSession<P>,
    binding: Option<&CaptureBinding>,
    gate: &mut impl FnMut(Stage) -> io::Result<()>,
    until: Instant,
) -> io::Result<DrainReport> {
    let reader = owned.reader.as_mut().ok_or_else(invalid)?;
    let mut writer = writer::Writer::new(raw, session, gate, binding, until);
    let (final_value, reconciled) = drain(reader, &mut writer, until)?;
    let progress = &final_value.progress;
    // Sticky reader loss leaves a finite contiguous FIFO prefix. A healthy owner
    // acknowledges that prefix before recording loss; storage uncertainty still
    // poisons immediately and abandons all later accepted batches.
    if progress.discard || progress.stdout.discarded > 0 || progress.stderr.discarded > 0 {
        writer.loss();
    }
    if !complete_final(&final_value, reconciled) {
        writer.poison();
    }
    // Original direct-child wait remains independent of capture status.
    let exit = match owned.child.wait_exit_until(until, || false) {
        Ok(WaitOutcome::Exited(exit)) => exit,
        _ => {
            writer.poison();
            return Err(invalid());
        }
    };
    let terminal = writer.finish(exit).is_ok();
    let accounting = writer.accounting;
    if (
        accounting.acknowledged.0 + accounting.abandoned.0 + accounting.uncertain.0,
        accounting.acknowledged.1 + accounting.abandoned.1 + accounting.uncertain.1,
    ) != reconciled
    {
        writer.poison();
        return Err(invalid());
    }
    Ok(DrainReport {
        terminal,
        accounting,
    })
}
fn drain<P: OwnerPort, G: FnMut(Stage) -> io::Result<()>>(
    reader: &mut hf_platform::process::PrivateFixtureCaptureReader,
    writer: &mut writer::Writer<'_, P, G>,
    until: Instant,
) -> io::Result<(ReaderFinal, (u64, u64))> {
    let mut reconciled = (0_u64, 0_u64);
    let mut final_value = None;
    loop {
        if Instant::now() >= until {
            writer.poison();
            reader.request_stop();
            return Err(invalid());
        }
        if writer.poisoned() {
            reader.discard_capture();
        }
        if reader.is_finished() && final_value.is_none() {
            final_value = Some(match reader.join_finished() {
                Ok(value) => value,
                Err(error) => {
                    writer.poison();
                    return Err(error);
                }
            });
        }
        match reader.recv_timeout(
            Duration::from_millis(20).min(until.saturating_duration_since(Instant::now())),
        ) {
            Ok(batch) => {
                reconciled.0 += batch.stdout.len() as u64;
                reconciled.1 += batch.stderr.len() as u64;
                if &batch.origin != reader.origin() {
                    writer.poison();
                }
                writer.append(&batch.stdout, &batch.stderr);
            }
            Err(_) if final_value.is_some() => break,
            Err(_) => {}
        }
    }
    Ok((final_value.ok_or_else(invalid)?, reconciled))
}
fn complete_final(value: &ReaderFinal, reconciled: (u64, u64)) -> bool {
    let p = &value.progress;
    !value.stopped
        && !value.limited
        && !p.discard
        && p.stdout.eof
        && p.stderr.eof
        && p.stdout.error.is_none()
        && p.stderr.error.is_none()
        && p.stdout.discarded == 0
        && p.stderr.discarded == 0
        && (p.stdout.enqueued, p.stderr.enqueued) == reconciled
        && (p.stdout.read, p.stderr.read) == reconciled
}
