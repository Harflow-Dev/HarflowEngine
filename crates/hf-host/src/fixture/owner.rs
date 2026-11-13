use super::*;
use hf_application::owner::OwnerSession;
use hf_domain::ownership::OwnerClaim;
use hf_platform::fixture::qualify_ledger;
use hf_storage::{Limits, Store};
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
    bootstrap_cwd(&cwd)?;
    let gate = gate_config(&prepared.root, &prepared.gate)?;
    if qualify_ledger(Path::new(&prepared.root))? != prepared.ledger {
        return Err(invalid());
    }
    let store = map(Store::open_with(
        &prepared.root,
        Limits::default(),
        until,
        hf_platform::lease::shared(),
        hf_storage::WriterOptions::suppressed(),
    ))?;
    let stored = map(store.ownership_observe(&invitation.operation, until))?.ok_or_else(invalid)?;
    let fresh = self_identity()?;
    map(hf_application::fixture::validate_prepared(
        &invitation,
        &prepared,
        &fresh,
        &stored,
    ))?;
    let owner = OwnerClaim {
        owner: invitation.owner.clone(),
        nonce: invitation.nonce.clone(),
        process: invitation.process.clone(),
    };
    let mut session =
        OwnerSession::new(store, prepared.request.clone(), owner, invitation.revision);
    // Keep the root lease alive until process death on uncertain admission.
    if admission(&prepared, until).is_err() || session.claim(until).is_err() {
        std::process::exit(2);
    }
    gate.reach(
        Stage::OwnerAfterClaim,
        &invitation.operation,
        &invitation.nonce,
        until,
    )
    .inspect_err(|_| session.poison())?;
    let mode = if prepared.fault == FixtureFault::OutputSaturation {
        SATURATION
    } else {
        EFFECT
    };
    let child = if prepared.fault == FixtureFault::UnknownChildIdentity {
        spawn_with(mode, &cwd, &UnknownIdentity, until)
    } else {
        spawn(mode, &cwd, until)
    };
    let child = match child {
        Ok(child) => child,
        Err(_) => std::process::exit(2),
    };
    let mut child = super::owned::OwnerChild::new(child, until);
    let result = launch(&mut child, &channel, &prepared, &mut session, &gate, until);
    if result.is_err() {
        session.poison();
    }
    let cleaned = child.settle();
    if cleaned.is_err() {
        std::process::exit(2);
    }
    match (result, cleaned) {
        (Ok(()), Ok(())) => Ok(()),
        _ => Err(invalid()),
    }
}
fn launch(
    owned: &mut super::owned::OwnerChild,
    channel: &PrivateIo,
    prepared: &Prepared,
    session: &mut OwnerSession<Store>,
    gate: &FixtureGate,
    until: Instant,
) -> io::Result<()> {
    let child = &mut owned.child;
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
    map(session.record_child(&physical, until))?;
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
            fence: fence(session)?,
        }),
        until,
    )?;
    child.close_stdin();
    // No transaction or io.lock spans this fixed child protocol wait.
    if child_receive(child, until)?
        != (Frame::EffectDone {
            operation: invitation.operation.clone(),
            process: physical.clone(),
        })
    {
        return Err(invalid());
    }
    let accepted = capture::complete(owned, prepared, &mut raw, session, gate, until)?;
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
            fence: accepted.clone(),
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

fn fence(session: &OwnerSession<Store>) -> io::Result<hf_domain::ownership::Fence> {
    session
        .snapshot()
        .and_then(|value| value.fence())
        .ok_or_else(invalid)
}
fn admission(prepared: &Prepared, until: Instant) -> io::Result<()> {
    let root = Path::new(&prepared.root);
    if prepared.fault == FixtureFault::OwnerCapacityExhausted {
        hf_platform::fixture::RawCapture::exhaust_fixture_capacity(root, until)?;
    }
    hf_platform::fixture::RawCapture::prepare_sentinels(root, &prepared.invitation.operation, until)
}
