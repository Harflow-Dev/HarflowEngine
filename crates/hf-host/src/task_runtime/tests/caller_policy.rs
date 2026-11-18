//! Caller-policy refusals of the dispatcher, and where they fall relative to root creation.
use super::{
    caller_source::{Effect, Scripted},
    harness::{Ctx, deadline, dispatcher_hooks, open_reader, request, require_clean_environment},
    serial_case,
};
use crate::task_runtime::{
    Error, dispatch_policy, dispatch_request, run_for_ctl_exit, seams, store_options,
};
use hf_application::ctl::task_run::RunFailure;
use hf_domain::ctl::state_root::StateRoot;
use hf_storage::{Limits, Store};
use std::{cell::Cell, rc::Rc};

fn run(ctx: &Ctx) -> Result<String, RunFailure> {
    let root = StateRoot::from_normalized(ctx.root().to_str().unwrap().into());
    run_for_ctl_exit(&root, &request(ctx))
}
fn assert_refused(result: Result<String, RunFailure>) {
    assert!(matches!(result, Err(RunFailure::Refused(_))), "{result:?}");
}
fn init_empty(ctx: &Ctx) {
    drop(
        Store::initialize_runtime(
            ctx.root(),
            Limits::default(),
            deadline(),
            hf_platform::lease::shared(),
            store_options::suppressed(),
        )
        .unwrap(),
    );
}
/// A hook at T0 that counts arrivals and fails them.
fn t0_counter() -> (Rc<Cell<usize>>, Rc<seams::DispatchHook>) {
    let count = Rc::new(Cell::new(0));
    let seen = count.clone();
    let hook = Rc::new(move |_: seams::DispatchPoint, _: &_| {
        seen.set(seen.get() + 1);
        Err(Error::Io(std::io::Error::other("injected failure")))
    });
    (count, hook)
}
fn candidates(ctx: &Ctx) -> usize {
    open_reader(&ctx.root())
        .runtime_mutation_candidates(
            &hf_interfaces::task_runtime::CanonicalLaunchValidator,
            64,
            deadline(),
        )
        .unwrap()
        .len()
}

#[test]
fn run_refuses_detected_agent_ancestry() {
    let _serial = serial_case();
    require_clean_environment();
    let ctx = Ctx::new("anc");
    // The seeded preparation records PP as a participant of an agent task.
    let seed = ctx.seed("read", hf_backends::backend_launch::CLAUDE_VERSION);
    let (count, hook) = t0_counter();
    let _hooks = dispatcher_hooks(&ctx, Some(hook));
    assert_refused(run(&ctx));
    assert_eq!(count.get(), 0, "no launch");
    let evidence = seed.evidence();
    assert_eq!(evidence.revision, 0);
    assert_eq!(candidates(&ctx), 1, "no new preparation rows");
    assert_eq!(ctx.fake.starts(), 0);
    seed.assert_never_accepted();
}

#[test]
fn run_refuses_unreadable_caller_inventory() {
    let _serial = serial_case();
    require_clean_environment();
    for errno in [1, 3] {
        let ctx = Ctx::new(&format!("inv{errno}"));
        let (count, hook) = t0_counter();
        let _hooks = dispatcher_hooks(&ctx, Some(hook));
        let parent = ctx.chain.parent.pid();
        ctx.scripted
            .rule(std::process::id(), 1, Effect::Fail(parent, errno));
        assert_refused(run(&ctx));
        assert!(!ctx.root().exists(), "no root");
        assert_eq!(count.get(), 0);
    }
}

#[test]
fn run_refuses_incomplete_ancestry() {
    let _serial = serial_case();
    require_clean_environment();
    let me = std::process::id();
    let (pp, _) = Scripted::pids();
    let cases: Vec<(&str, usize, Effect)> = vec![
        ("loop", 1, Effect::Reparent(pp, me)),
        ("boot", 1, Effect::BootFlip),
        ("short", 1, Effect::Reparent(me, 1)),
        ("recheck", 2, Effect::Restart(0)),
    ];
    for (name, nth, effect) in cases {
        let ctx = Ctx::new(&format!("inc-{name}"));
        let (count, hook) = t0_counter();
        let _hooks = dispatcher_hooks(&ctx, Some(hook));
        let effect = match effect {
            Effect::Restart(_) => Effect::Restart(ctx.chain.parent.pid()),
            other => other,
        };
        ctx.scripted.rule(me, nth, effect);
        assert_refused(run(&ctx));
        assert!(!ctx.root().exists(), "{name}: no root");
        assert_eq!(count.get(), 0, "{name}");
    }
}

#[test]
fn run_refuses_caller_change_at_final_observation() {
    let _serial = serial_case();
    require_clean_environment();
    let me = std::process::id();
    // Missing root: the change is seen only after the root was created and initialized.
    let ctx = Ctx::new("final-fresh");
    let (count, hook) = t0_counter();
    let _hooks = dispatcher_hooks(&ctx, Some(hook));
    ctx.scripted
        .rule(me, 3, Effect::Restart(ctx.chain.parent.pid()));
    assert_refused(run(&ctx));
    assert_eq!(count.get(), 0, "no T0, probe or spawn");
    assert!(ctx.root().exists(), "an initialized root remains");
    assert_eq!(candidates(&ctx), 0, "and it is empty");
    // Existing empty root.
    let ctx = Ctx::new("final-existing");
    init_empty(&ctx);
    let (count, hook) = t0_counter();
    let _hooks = dispatcher_hooks(&ctx, Some(hook));
    ctx.scripted
        .rule(me, 3, Effect::Restart(ctx.chain.parent.pid()));
    assert_refused(run(&ctx));
    assert_eq!(count.get(), 0);
    assert_eq!(candidates(&ctx), 0);
}

#[test]
fn ordinary_shell_parent_is_not_agent_evidence() {
    let _serial = serial_case();
    require_clean_environment();
    let ctx = Ctx::new("shell");
    let _hooks = dispatcher_hooks(&ctx, None);
    let admitted = dispatch_request::admit(&request(&ctx), deadline()).expect("admission");
    assert!(!ctx.root().exists(), "admission writes nothing");
    // An existing unrelated root.
    init_empty(&ctx);
    let store = open_reader(&ctx.root());
    dispatch_policy::inventory(&ctx.root(), &store, &admitted.caller, deadline())
        .expect("inventory");
    assert_eq!(candidates(&ctx), 0, "no T0");
}
