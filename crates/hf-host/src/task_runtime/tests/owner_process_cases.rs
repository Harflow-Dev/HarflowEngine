//! Owner cases about process custody: descendants, early exit and malformed output.
use super::{
    ack_loss::{failure_of, is_unknown, retain_with},
    harness::{Act, Ctx, Outcome, Script, WriterConfig, drive_case, start_owner, with_case},
    owner_cases::{exited, wait_for},
    serial_case,
};
use crate::task_runtime::seams::{self, OwnerPoint};
use hf_application::process::IdentityPort as _;
use hf_domain::ownership::Phase;
use hf_platform::process::MacIdentity;
use std::{rc::Rc, time::Duration};

const VERSION: &str = hf_backends::backend_launch::CLAUDE_VERSION;

#[test]
fn held_descendant_pipe_does_not_fake_eof_or_completion() {
    let _serial = serial_case();
    let ctx = Ctx::new("descendant");
    let seed = ctx.seed("read,init,descendant,gate:a,text,result,exit:0", VERSION);
    let script = Rc::new(Script::default());
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let (_, reply) = with_case(&ctx, case, |case| {
        let runtime = &mut case.runtime;
        runtime.initialize().expect("owner start");
        wait_for("descendant pid", Duration::from_secs(20), || {
            ctx.fake.descendant_pid().is_some()
        });
        let pid = ctx.fake.descendant_pid().unwrap();
        let identity = MacIdentity.identity(pid).expect("descendant identity");
        assert!(
            ctx.cleanup
                .gates
                .borrow_mut()
                .connect("desc", Duration::from_secs(20))
        );
        ctx.cleanup.register(identity.clone());
        // The parent's result and exit are gated; then it exits holding nothing, while the
        // descendant still holds the pipe.
        ctx.cleanup.gates.borrow_mut().open("a");
        wait_for("agent exit", Duration::from_secs(20), || exited(runtime));
        for _ in 0..200 {
            assert!(
                !runtime.tick().unwrap(),
                "no terminal while the pipe is held"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(seed.task_status().as_deref(), Some("running"));
        ctx.cleanup.gates.borrow_mut().close("desc");
        wait_for("descendant exit", Duration::from_secs(5), || {
            MacIdentity.identity(pid).as_ref() != Some(&identity)
        });
        wait_for("settlement", Duration::from_secs(5), || {
            runtime.tick().unwrap()
        });
        runtime.settle_writer().unwrap();
    });
    drop(reply);
    assert_eq!(seed.task_status().as_deref(), Some("completed"));
    seed.assert_never_accepted();
}

#[test]
fn task_never_records_acceptance_recorded_or_cancelled() {
    let _serial = serial_case();
    for (name, steps, expected) in [
        (
            "accept-ok",
            "read,init,text,result,eofwait,exit:0",
            "completed",
        ),
        (
            "accept-bad",
            "read,init,text,result_fail,eofwait,exit:1",
            "failed",
        ),
    ] {
        let ctx = Ctx::new(name);
        let seed = ctx.seed(steps, VERSION);
        seed.assert_never_accepted();
        let mut script = Rc::new(Script::default());
        for point in [
            OwnerPoint::AfterT1,
            OwnerPoint::AfterT2,
            OwnerPoint::BeforeStarted,
        ] {
            let probe = seed.probe.clone();
            let check = Act::Pause(Box::new(move |_| probe.assert_never_accepted()));
            script = script.on(point, check);
        }
        let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
        let (outcome, _) = with_case(&ctx, case, |case| drive_case(&mut case.runtime));
        assert_eq!(outcome, Outcome::Settled(0));
        seed.assert_never_accepted();
        assert_eq!(seed.task_status().as_deref(), Some(expected));
    }
}

#[test]
fn agent_exit_before_stdin_read_is_unknown_and_retained() {
    let _serial = serial_case();
    let ctx = Ctx::new("exit-early");
    let seed = ctx.seed("exit:0", VERSION);
    let pause = Act::Pause(Box::new(|runtime| {
        wait_for("agent exit", Duration::from_secs(20), || exited(runtime));
    }));
    let script = Rc::new(Script::default()).on(OwnerPoint::AfterT2, pause);
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let _owner = seams::install_owner(script.hook());
    let (found, reply) = retain_with(&ctx, &seed, &script, case);
    assert_eq!((found.phase, found.revision), (Phase::SpawnObserved, 2));
    assert_eq!(found.task.as_deref(), Some("running"));
    assert!(found.agent && found.reader);
    assert!(is_unknown(&failure_of(reply)));
    seed.assert_never_accepted();
}

#[test]
fn owner_level_malformed_output_reaches_a_terminal_after_stdin_closes() {
    let _serial = serial_case();
    let ctx = Ctx::new("owner-malformed");
    let seed = ctx.seed("read,init,garbage,eofwait,exit:0", VERSION);
    let script = Rc::new(Script::default());
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let (outcome, _) = with_case(&ctx, case, |case| drive_case(&mut case.runtime));
    assert_eq!(outcome, Outcome::Settled(0));
    assert_eq!(seed.task_status().as_deref(), Some("failed"));
    assert!(seed.evidence().observation.observation_gap);
    seed.assert_never_accepted();
}
