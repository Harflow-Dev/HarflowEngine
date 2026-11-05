use super::*;
use hf_domain::ownership::{ClaimResult, Snapshot};
use hf_platform::fixture::qualify_ledger;
pub(super) fn run(channel: PrivateIo, until: Instant) -> io::Result<()> {
    let actual = self_identity()?;
    send(&channel, &Frame::Hello(actual.clone()))?;
    let Frame::Invitation(invitation) = receive(&channel)? else {
        return Err(invalid());
    };
    if invitation.process != actual {
        return Err(invalid());
    }
    send(&channel, &Frame::Ready(invitation.clone()))?;
    let Frame::Prepared(prepared) = receive(&channel)? else {
        return Err(invalid());
    };
    channel.expect_eof()?;
    let cwd = root_parent(&prepared.root)?;
    let gate = gate_config(&prepared.root, &prepared.gate)?;
    if qualify_ledger(Path::new(&prepared.root))? != prepared.ledger {
        return Err(invalid());
    }
    let commands = store::Commands::new(&prepared.root, until);
    let stored = commands.observe(&invitation.operation)?;
    let fresh = self_identity()?;
    map(hf_application::fixture::validate_prepared(
        &invitation,
        &prepared,
        &fresh,
        &stored,
    ))?;
    let ClaimResult::Granted(claimed) = commands.claim(&invitation)? else {
        return Err(invalid());
    };
    // Commands retains a path/deadline only. All Store handles are already closed.
    gate.reach(
        Stage::OwnerAfterClaim,
        &invitation.operation,
        &invitation.nonce,
        until,
    )?;
    let mut child = if prepared.fault == FixtureFault::UnknownChildIdentity {
        spawn_with(EFFECT, &cwd, &UnknownIdentity, until)?
    } else {
        spawn(EFFECT, &cwd, until)?
    };
    let result = launch(&mut child, &channel, &prepared, &claimed, &gate, until);
    let cleaned = cleanup(&mut child);
    match (result, cleaned) {
        (Ok(()), Ok(())) => Ok(()),
        _ => Err(invalid()),
    }
}
fn launch(
    child: &mut RetainedOwnedChild,
    channel: &PrivateIo,
    prepared: &Prepared,
    claimed: &Snapshot,
    gate: &FixtureGate,
    until: Instant,
) -> io::Result<()> {
    if prepared.fault == FixtureFault::UnusableChildPipe {
        return Err(invalid());
    }
    let physical = ready(child)?;
    let invitation = &prepared.invitation;
    gate.reach(
        Stage::OwnerAfterChildSpawn,
        &invitation.operation,
        &invitation.nonce,
        until,
    )?;
    let fence = claimed.fence().ok_or_else(invalid)?;
    let observed = store::Commands::new(&prepared.root, until).observed(&fence, &physical)?;
    gate.reach(
        Stage::OwnerAfterObserved,
        &invitation.operation,
        &invitation.nonce,
        until,
    )?;
    let mut raw = hf_platform::fixture::RawCapture::create(
        Path::new(&prepared.root),
        &invitation.operation,
        until,
    )?;
    child_send(
        child,
        &Frame::Start(EffectStart {
            operation: invitation.operation.clone(),
            root: prepared.root.clone(),
            ledger: prepared.ledger,
            fence: observed.fence().ok_or_else(invalid)?,
        }),
        until,
    )?;
    child.close_stdin();
    // Independent fixed child open/probe/write has finished and closed its Store
    // before the owner opens again. No storage handle waits for this response.
    if child_receive(child, until)?
        != (Frame::EffectDone {
            operation: invitation.operation.clone(),
            process: physical.clone(),
        })
    {
        return Err(invalid());
    }
    let accepted = store::Commands::new(&prepared.root, until)
        .accept(&observed.fence().ok_or_else(invalid)?)?;
    let (stdout, stderr) = raw.identities();
    capture::complete(
        child,
        prepared,
        hf_domain::capture::CaptureBinding {
            fence: accepted.fence().ok_or_else(invalid)?,
            child: physical.clone(),
            stdout,
            stderr,
        },
        &mut raw,
        gate,
        until,
    )?;
    gate.reach(
        Stage::OwnerBeforeAck,
        &invitation.operation,
        &invitation.nonce,
        until,
    )?;
    send(
        channel,
        &Frame::Accepted {
            invitation: invitation.clone(),
            fence: accepted.fence().ok_or_else(invalid)?,
            child: physical,
        },
    )
}
struct UnknownIdentity;
impl IdentityPort for UnknownIdentity {
    fn identity(&self, _pid: u32) -> Option<ProcessIdentity> {
        None
    }
}
