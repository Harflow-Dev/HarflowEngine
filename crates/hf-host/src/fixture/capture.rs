//! Single-owner bounded capture after the exact framed prefix has been consumed.
use super::*;
use hf_domain::{capture::CaptureBinding, process::ProcessExit};
use hf_platform::fixture::RawCapture;
pub(super) fn complete(
    child: &mut RetainedOwnedChild,
    prepared: &Prepared,
    binding: CaptureBinding,
    raw: &mut RawCapture,
    gate: &FixtureGate,
    until: Instant,
) -> io::Result<()> {
    let commands = store::Commands::new(&prepared.root, until);
    let mut snapshot = commands.capture_begin(&binding)?;
    let mut append_gates = true;
    let mut publish_gate = true;
    loop {
        if Instant::now() >= until {
            return Err(invalid());
        }
        child.pump(until, 32768)?;
        let (stdout, stderr) = child.take_capture_bytes();
        let state = child.capture();
        if state.stdout.overflow
            || state.stderr.overflow
            || state.stdout.error.is_some()
            || state.stderr.error.is_some()
        {
            return Err(invalid());
        }
        let mut next = snapshot.progress.clone();
        let changed = !stdout.is_empty() || !stderr.is_empty();
        if changed {
            let offsets =
                raw.append(next.stdout, next.stderr, &stdout, &stderr, until, |stage| {
                    if append_gates {
                        reach(gate, stage, prepared, until)
                    } else {
                        Ok(())
                    }
                })?;
            append_gates = false;
            next.stdout = offsets.stdout;
            next.stderr = offsets.stderr;
        }
        next.stdout_eof = state.stdout.eof;
        next.stderr_eof = state.stderr.eof;
        if next != snapshot.progress {
            next.revision = next.revision.checked_add(1).ok_or_else(invalid)?;
            snapshot = commands.capture_publish(&binding, snapshot.progress.revision, &next)?;
            if publish_gate {
                reach(gate, Stage::CaptureAfterPublish, prepared, until)?;
                publish_gate = false;
            }
        }
        let exit = child.try_wait()?;
        if snapshot.progress.stdout_eof
            && snapshot.progress.stderr_eof
            && matches!(exit, ProcessExit::Code(_) | ProcessExit::Signal(1..))
        {
            reach(gate, Stage::CaptureBeforeTerminal, prepared, until)?;
            raw.validate(snapshot.progress.stdout, snapshot.progress.stderr, until)?;
            commands.capture_finish(&binding, snapshot.progress.revision, exit)?;
            return reach(gate, Stage::CaptureAfterTerminal, prepared, until);
        }
    }
}
fn reach(gate: &FixtureGate, stage: Stage, prepared: &Prepared, until: Instant) -> io::Result<()> {
    gate.reach(
        stage,
        &prepared.invitation.operation,
        &prepared.invitation.nonce,
        until,
    )
}
