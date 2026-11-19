//! Lost acknowledgements around T0, T1, T2 and the Started reply.
use super::{
    harness::{
        Act, Ctx, Held, Outcome, OwnerCase, Script, Seed, WriterConfig, deadline, dispatcher_hooks,
        drive_case, held, request, require_clean_environment, start_owner, with_case,
    },
    observer::observe,
    serial_case,
};
use crate::task_runtime::{
    Error,
    dispatch::classify,
    dispatch_child::Reply,
    run_for_ctl_exit,
    seams::{self, OwnerPoint},
};
use hf_application::ctl::task_run::RunFailure;
use hf_domain::{ctl::state_root::StateRoot, ownership::Phase};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const STEPS: &str = "read,init,text,result,eofwait,exit:0";
const VERSION: &str = hf_backends::backend_launch::CLAUDE_VERSION;

pub(super) fn is_unknown(failure: &RunFailure) -> bool {
    matches!(failure, RunFailure::Unknown(_))
}
/// The dispatcher's reading of a lost or refused exchange.
pub(super) fn failure_of(reply: Option<Result<Reply, Error>>) -> RunFailure {
    match reply.expect("dispatcher exchange") {
        Err(error) => classify(error),
        Ok(Reply::NotStarted(reason)) => classify(Error::NotStarted(reason)),
        Ok(Reply::Started(task)) => panic!("unexpected Started {task}"),
    }
}
/// Starts an owner whose retention is observed; returns what it held when it retained.
pub(super) fn retain_with(
    ctx: &Ctx,
    seed: &Seed,
    script: &Rc<Script>,
    case: OwnerCase,
) -> (Held, Option<Result<Reply, Error>>) {
    let probe = seed.probe.clone();
    let (hook, findings) = observe(
        ctx.cleanup.clone(),
        Duration::from_secs(30),
        |_| {},
        move |rt| Some(held(rt, &probe)),
    );
    let _retained = seams::install_owner_retained(hook);
    let _ = script;
    let (outcome, reply) = with_case(ctx, case, |case| drive_case(&mut case.runtime));
    assert_eq!(outcome, Outcome::Retained);
    (findings.take(), reply)
}

#[test]
fn ack_loss_after_t0_requires_positive_no_spawn_proof() {
    let _serial = serial_case();
    require_clean_environment();
    let ctx = Ctx::new("t0");
    let seen = Rc::new(RefCell::new(None));
    let capture = seen.clone();
    let fault = Rc::new(move |point: seams::DispatchPoint, binding: &_| {
        if point != seams::DispatchPoint::AfterT0 {
            return Ok(());
        }
        *capture.borrow_mut() = Some(hf_domain::task_lifecycle::LifecycleBinding::clone(binding));
        Err(Error::Io(std::io::Error::other("injected failure")))
    });
    let _hooks = dispatcher_hooks(&ctx, Some(fault));
    let root = StateRoot::from_normalized(ctx.root().to_str().unwrap().into());
    let failure = run_for_ctl_exit(&root, &request(&ctx)).unwrap_err();
    assert!(is_unknown(&failure), "{failure:?}");
    let probe = super::harness::Probe {
        root: ctx.root(),
        binding: seen.borrow().clone().expect("T0 was written"),
    };
    let evidence = probe.evidence();
    assert_eq!(
        (evidence.ownership.phase, evidence.revision),
        (Phase::Prepared, 0)
    );
    assert!(evidence.ownership.owner.is_none() && evidence.ownership.child.is_none());
    assert!(
        probe
            .store()
            .read_launch_intent(&probe.binding, deadline())
            .unwrap()
            .is_some()
    );
    assert_eq!(probe.task_status(), None, "no task row");
    probe.assert_never_accepted();
    assert_eq!(ctx.fake.starts(), 0, "launch was not reached");
    // Only a decoded NotStarted is a positive refusal; every other loss is unknown.
    assert!(matches!(
        classify(Error::NotStarted("unsupported_version".into())),
        RunFailure::Refused(_)
    ));
    for error in [
        Error::Refused,
        Error::Poisoned,
        Error::Io(std::io::Error::other("x")),
    ] {
        assert!(is_unknown(&classify(error)));
    }
}

#[test]
fn ack_loss_after_t1_reports_unknown_and_never_relaunches() {
    let _serial = serial_case();
    let ctx = Ctx::new("t1");
    let seed = ctx.seed(STEPS, VERSION);
    let script = Rc::new(Script::default()).on(OwnerPoint::AfterT1, Act::Fail);
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let _owner = seams::install_owner(script.hook());
    let (found, reply) = retain_with(&ctx, &seed, &script, case);
    assert_eq!((found.phase, found.revision), (Phase::Claimed, 1));
    assert!(found.writer && !found.agent && !found.reader && !found.discovery);
    assert_eq!(found.task, None);
    assert_eq!(ctx.fake.starts(), 0);
    assert_eq!(script.arrivals(OwnerPoint::AfterT1), 1);
    assert!(is_unknown(&failure_of(reply)));
    seed.assert_never_accepted();
}

#[test]
fn unsupported_version_is_a_positive_refusal() {
    let _serial = serial_case();
    let ctx = Ctx::new("t1-contrast");
    let seed = ctx.seed(STEPS, "2.0.0");
    let script = Rc::new(Script::default());
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let _owner = seams::install_owner(script.hook());
    let (outcome, reply) = with_case(&ctx, case, |case| drive_case(&mut case.runtime));
    assert_eq!(outcome, Outcome::Settled(0));
    assert!(matches!(failure_of(reply), RunFailure::Refused(_)));
    assert_eq!(ctx.fake.starts(), 0);
    assert_eq!(seed.task_status(), None);
    seed.assert_never_accepted();
}

#[test]
fn ack_loss_after_t2_reports_unknown_and_owner_retains_custody() {
    let _serial = serial_case();
    let ctx = Ctx::new("t2");
    let seed = ctx.seed(STEPS, VERSION);
    let script = Rc::new(Script::default()).on(OwnerPoint::AfterT2, Act::Fail);
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let _owner = seams::install_owner(script.hook());
    let (found, reply) = retain_with(&ctx, &seed, &script, case);
    assert_eq!((found.phase, found.revision), (Phase::SpawnObserved, 2));
    assert_eq!(found.task.as_deref(), Some("running"));
    assert!(found.agent && found.reader && found.writer);
    assert!(is_unknown(&failure_of(reply)));
    seed.assert_never_accepted();
}

#[test]
fn ack_loss_around_started_send_effects_once() {
    started_fails();
    started_dispatcher_gone();
    started_normal();
}

fn started_fails() {
    let _serial = serial_case();
    let ctx = Ctx::new("s1");
    let seed = ctx.seed(STEPS, VERSION);
    let script = Rc::new(Script::default()).on(OwnerPoint::BeforeStarted, Act::Fail);
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let _owner = seams::install_owner(script.hook());
    let (found, reply) = retain_with(&ctx, &seed, &script, case);
    assert_eq!((found.phase, found.revision), (Phase::SpawnObserved, 2));
    assert_eq!(found.task.as_deref(), Some("running"));
    assert!(found.agent);
    assert!(is_unknown(&failure_of(reply)));
    seed.assert_never_accepted();
}

/// The test is the dispatcher: it drops its channel end while the owner is paused before
/// the Started send, so that send fails.
fn started_dispatcher_gone() {
    let _serial = serial_case();
    let ctx = Ctx::new("s2");
    let seed = ctx.seed(STEPS, VERSION);
    let (arrived, release) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let (a, r) = (arrived.clone(), release.clone());
    let pause = Act::Pause(Box::new(move |_| {
        a.store(true, Ordering::Release);
        let end = Instant::now() + Duration::from_secs(20);
        while !r.load(Ordering::Acquire) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(2));
        }
    }));
    let script = Rc::new(Script::default()).on(OwnerPoint::BeforeStarted, pause);
    let case = start_owner(
        &ctx,
        &seed,
        &WriterConfig::default(),
        &script,
        Some((arrived, release)),
    );
    let _owner = seams::install_owner(script.hook());
    let (found, _) = retain_with(&ctx, &seed, &script, case);
    assert_eq!((found.phase, found.revision), (Phase::SpawnObserved, 2));
    assert_eq!(found.task.as_deref(), Some("running"));
    assert!(found.agent && found.reader);
    assert_eq!(script.arrivals(OwnerPoint::BeforeStarted), 1);
    seed.assert_never_accepted();
}

fn started_normal() {
    let _serial = serial_case();
    let ctx = Ctx::new("s3");
    let seed = ctx.seed(STEPS, VERSION);
    let script = Rc::new(Script::default());
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let _owner = seams::install_owner(script.hook());
    let (outcome, reply) = with_case(&ctx, case, |case| drive_case(&mut case.runtime));
    assert_eq!(outcome, Outcome::Settled(0));
    match reply.expect("dispatcher exchange") {
        Ok(Reply::Started(task)) => assert_eq!(task, seed.binding.task.as_str()),
        other => panic!("expected Started, got {other:?}"),
    }
    assert_eq!(seed.task_status().as_deref(), Some("completed"));
    assert_eq!(ctx.fake.starts(), 1);
    seed.assert_never_accepted();
}
