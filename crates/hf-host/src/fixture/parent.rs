use super::*;
use hf_domain::ownership::PrepareResult;
use hf_platform::fixture::qualify_ledger;
pub(super) fn run(channel: PrivateIo, until: Instant) -> io::Result<()> {
    let Frame::Parent(mut plan) = receive(&channel)? else {
        return Err(invalid());
    };
    channel.expect_eof()?;
    let result = execute(&mut plan, until).unwrap_or_else(|_| {
        report(
            plan.request.operation.clone(),
            plan.request.generation.clone(),
            ReportKind::Uncertain,
        )
    });
    send(&channel, &result)
}
fn execute(plan: &mut ParentPlan, until: Instant) -> io::Result<Frame> {
    let cwd = root_parent(&plan.root)?;
    bootstrap_cwd(&cwd)?;
    let gate = gate_config(&plan.root, &plan.gate)?;
    if qualify_ledger(Path::new(&plan.root))? != plan.ledger {
        return Err(invalid());
    }
    if plan.initialize {
        plan.request.generation = store::Commands::new(&plan.root, until).initialize()?;
        plan.initialize = false;
    }
    gate.reach(
        Stage::ParentBeforePrepare,
        &plan.request.operation,
        &plan.nonce,
        until,
    )?;
    let prepared = store::Commands::new(&plan.root, until).prepare(plan)?;
    let kind = match prepared {
        PrepareResult::Existing(_) => Some(ReportKind::Existing),
        PrepareResult::Prepared(_) if !plan.launch => Some(ReportKind::Prepared),
        PrepareResult::Prepared(_) => None,
    };
    gate.reach(
        Stage::ParentAfterPrepare,
        &plan.request.operation,
        &plan.nonce,
        until,
    )?;
    if let Some(kind) = kind {
        return Ok(report(
            plan.request.operation.clone(),
            plan.request.generation.clone(),
            kind,
        ));
    }
    gate.reach(
        Stage::ParentBeforeSpawn,
        &plan.request.operation,
        &plan.nonce,
        until,
    )?;
    let mut child = spawn(OWNER, &cwd, until)?;
    // Every path after spawn goes through cleanup, including unknown initialization.
    let result = handoff(&mut child, plan, &gate, until);
    let cleaned = cleanup(&mut child);
    match (result, cleaned) {
        (Ok(frame), Ok(())) => Ok(frame),
        _ => Err(invalid()),
    }
}
fn handoff(
    child: &mut RetainedOwnedChild,
    plan: &ParentPlan,
    gate: &FixtureGate,
    until: Instant,
) -> io::Result<Frame> {
    let actual = ready(child)?;
    gate.reach(
        Stage::ParentAfterOwnerSpawn,
        &plan.request.operation,
        &plan.nonce,
        until,
    )?;
    if child_receive(child, until)? != Frame::Hello(actual.clone()) {
        return Err(invalid());
    }
    let invitation = Invitation {
        operation: plan.request.operation.clone(),
        generation: plan.request.generation.clone(),
        digest: plan.request.digest.clone(),
        owner: plan.owner.clone(),
        nonce: plan.nonce.clone(),
        process: actual.clone(),
        revision: 0,
    };
    child_send(child, &Frame::Invitation(invitation.clone()), until)?;
    if child_receive(child, until)? != Frame::Ready(invitation.clone()) {
        return Err(invalid());
    }
    child_send(
        child,
        &Frame::Prepared(Prepared {
            invitation: invitation.clone(),
            request: plan.request.clone(),
            root: plan.root.clone(),
            gate: plan.gate.clone(),
            ledger: plan.ledger,
            fault: plan.fault,
        }),
        until,
    )?;
    child.close_stdin();
    let frame = child_receive(child, until)?;
    let Frame::Accepted {
        invitation: echoed,
        fence,
        child: physical,
    } = &frame
    else {
        return Err(invalid());
    };
    if echoed != &invitation {
        return Err(invalid());
    }
    let stored = store::Commands::new(&plan.root, until).observe(&plan.request.operation)?;
    if stored.request != plan.request {
        return Err(invalid());
    }
    map(hf_application::fixture::validate_accepted(
        &invitation,
        fence,
        physical,
        &actual,
        &stored,
    ))?;
    Ok(frame)
}
