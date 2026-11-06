use super::*;
use hf_domain::ownership::PrepareResult;
use hf_platform::fixture::qualify_ledger;
pub(super) fn run(channel: PrivateIo, until: Instant, qualification: bool) -> io::Result<()> {
    let Frame::Parent(mut plan) = receive(&channel)? else {
        return Err(invalid());
    };
    channel.expect_eof()?;
    let result = execute(&mut plan, until, qualification).unwrap_or_else(|_| {
        report(
            plan.request.operation.clone(),
            plan.request.generation.clone(),
            ReportKind::Uncertain,
        )
    });
    send(&channel, &result)
}
fn validate_mode(plan: &ParentPlan, qualification: bool) -> io::Result<bool> {
    let cq = plan.request.payload.starts_with(b"HFCQ");
    if cq != qualification {
        return Err(invalid());
    }
    let fixed = plan.request.payload.starts_with(b"HFPL") || cq;
    if fixed {
        if plan.initialize {
            return Err(invalid());
        }
        let launch = map(hf_interfaces::prepared_launch::decode_preparation(
            &plan.request,
            &hf_storage::Sha256Digest,
        ))?;
        if hf_platform::process::MacIdentity
            .identity(std::process::id())
            .as_ref()
            != Some(&launch.policy().authority.caller)
        {
            return Err(invalid());
        }
    }
    Ok(fixed)
}
fn execute(plan: &mut ParentPlan, until: Instant, qualification: bool) -> io::Result<Frame> {
    let cq = plan.request.payload.starts_with(b"HFCQ");
    let fixed = validate_mode(plan, qualification)?;
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
    validate_fixed(plan, fixed, &cwd, until)?;
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
    let mut child = spawn(
        if cq {
            hf_domain::claude_qualification::OWNER_MODE
        } else if fixed {
            "--internal-isolated-owner-fixture-v1"
        } else {
            OWNER
        },
        &cwd,
        until,
    )?;
    // Every path after spawn goes through cleanup, including unknown initialization.
    let result = handoff(&mut child, plan, &gate, until);
    let cleaned = if fixed {
        super::retention::settle(&mut child)
    } else {
        cleanup(&mut child)
    };
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
    let frame =
        if plan.request.payload.starts_with(b"HFPL") || plan.request.payload.starts_with(b"HFCQ") {
            captured_frame(child, plan, &invitation, until)?
        } else {
            child_receive(child, until)?
        };
    if matches!(frame, Frame::CaptureAccepted(_)) {
        return validate_receipt(frame, plan, &invitation, &actual, until);
    }
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

fn echoed_owner(receipt: &hf_domain::prepared_launch::CaptureAccepted) -> Option<&ProcessIdentity> {
    receipt.ownership.owner.as_ref().map(|o| &o.process)
}

fn validate_fixed(plan: &ParentPlan, fixed: bool, cwd: &Path, until: Instant) -> io::Result<()> {
    if fixed {
        let decoded = map(hf_interfaces::prepared_launch::decode_preparation(
            &plan.request,
            &hf_storage::Sha256Digest,
        ))?;
        let (expected_source, expected_binary, expected_cwd, expected_ledger) = match &decoded {
            hf_interfaces::prepared_launch::DecodedPreparation::FixedProtocol(v) => {
                (&v.source, &v.binary, &v.cwd, v.ledger)
            }
            hf_interfaces::prepared_launch::DecodedPreparation::ClaudeQualification(v) => {
                super::claude_bridge::validate(v, until)?;
                (&v.source, &v.gate_binary, &v.cwd, v.ledger)
            }
            _ => return Err(invalid()),
        };
        let hold = map(hf_storage::RootHold::open(
            &plan.root,
            until,
            hf_platform::lease::shared(),
        ))?;
        let binary = map(hf_platform::prepared_launch::observe_binary(
            &std::env::current_exe()?,
            &hf_storage::Sha256Digest,
            until,
        ))?;
        if hold.source_stamp() != expected_source
            || &binary != expected_binary
            || Path::new(expected_cwd) != cwd
            || expected_ledger != plan.ledger
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn validate_receipt(
    frame: Frame,
    plan: &ParentPlan,
    invitation: &Invitation,
    actual: &ProcessIdentity,
    until: Instant,
) -> io::Result<Frame> {
    if let Frame::CaptureAccepted(receipt) = &frame {
        if !(plan.request.payload.starts_with(b"HFPL") || plan.request.payload.starts_with(b"HFCQ"))
            || echoed_owner(receipt) != Some(actual)
        {
            return Err(invalid());
        }
        if &receipt.invitation != invitation
            || receipt.ownership.request != plan.request
            || receipt.source.generation != plan.request.generation
            || receipt.capture.exit != hf_domain::process::ProcessExit::Code(0)
        {
            return Err(invalid());
        }
        let stored = store::Commands::new(&plan.root, until).observe(&plan.request.operation)?;
        if stored != receipt.ownership {
            return Err(invalid());
        }
        return Ok(frame);
    }
    Err(invalid())
}

fn captured_frame(
    child: &RetainedOwnedChild,
    plan: &ParentPlan,
    invitation: &Invitation,
    until: Instant,
) -> io::Result<Frame> {
    use hf_application::prepared_launch::{CaptureContext, ExactCaptureReader};
    let decoded = map(hf_interfaces::prepared_launch::decode_preparation(
        &plan.request,
        &hf_storage::Sha256Digest,
    ))?;
    let source_stamp = match &decoded {
        hf_interfaces::prepared_launch::DecodedPreparation::FixedProtocol(v) => &v.source,
        hf_interfaces::prepared_launch::DecodedPreparation::ClaudeQualification(v) => &v.source,
        _ => return Err(invalid()),
    };
    let parent = root_parent(&plan.root)?;
    let reader = ExactCaptureReader::new(
        hf_storage::RecoveryReadSession::new(
            &plan.root,
            parent,
            hf_storage::Limits::default(),
            hf_platform::lease::shared(),
        ),
        hf_platform::recovery::ExactRawCaptureReader::new(&plan.root),
        hf_storage::Sha256Digest,
    );
    let delivery = super::receipt::Delivery::new(child);
    let source = map(reader.read(
        CaptureContext {
            invitation,
            request: &plan.request,
            source: source_stamp,
        },
        &delivery,
        until,
    ))?;
    Ok(Frame::CaptureAccepted(Box::new(source.receipt)))
}
