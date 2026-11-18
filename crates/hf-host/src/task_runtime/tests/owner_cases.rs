//! The real owner loop on the test thread, with a scripted agent and a writer thread.
use super::{
    driver::{SESSION, init_line, text_line},
    harness::{Act, Ctx, Outcome, Script, WriterConfig, drive_case, start_owner, with_case},
    observer::observe,
    serial_case,
};
use crate::task_runtime::{
    dispatch_child::Reply,
    owner_runtime::Runtime,
    seams::{self, OwnerPoint},
    stream::Stream,
    writer_slot::WriterSlot,
};
use hf_domain::{process::ProcessExit, task_lifecycle::ExecutionObservation};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const VERSION: &str = hf_backends::backend_launch::CLAUDE_VERSION;

pub(super) fn wait_for(what: &str, within: Duration, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + within;
    while !ready() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}
pub(super) fn exited(runtime: &mut Runtime) -> bool {
    runtime.agent.as_mut().unwrap().try_wait().unwrap() != ProcessExit::Unobserved
}
pub(super) fn started(reply: Option<Result<Reply, crate::task_runtime::Error>>) -> String {
    match reply.expect("dispatcher exchange") {
        Ok(Reply::Started(task)) => task,
        other => panic!("expected Started, got {other:?}"),
    }
}

#[test]
fn short_completion_before_started_returns_same_task_id_and_terminal_state() {
    let _serial = serial_case();
    let ctx = Ctx::new("short");
    let seed = ctx.seed("read,init,text,result,exit:0", VERSION);
    let pause = Act::Pause(Box::new(|runtime| {
        wait_for("agent exit", Duration::from_secs(20), || exited(runtime));
    }));
    let script = Rc::new(Script::default()).on(OwnerPoint::BeforeStarted, pause);
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let (outcome, reply) = with_case(&ctx, case, |case| drive_case(&mut case.runtime));
    assert_eq!(outcome, Outcome::Settled(0));
    assert_eq!(started(reply), seed.binding.task.as_str());
    assert_eq!(seed.task_status().as_deref(), Some("completed"));
    seed.assert_never_accepted();
}

#[test]
fn writer_loss_poisons_mutation_and_keeps_storage_free_drain() {
    let _serial = serial_case();
    let ctx = Ctx::new("writer-loss");
    let seed = ctx.seed("read,gate:a,init,gate:b,text,result,exit:0", VERSION);
    // The writer is lost before the first output exists; the first output then fails the
    // exchange. The agent is released only after the loss.
    let released = ctx.cleanup.clone();
    let sever = Act::Pause(Box::new(move |runtime| {
        if let Some(WriterSlot::Thread(thread)) = &mut runtime.writer {
            thread.sever();
        }
        released.gates.borrow_mut().open("a");
    }));
    let script = Rc::new(Script::default()).on(OwnerPoint::BeforeStarted, sever);
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let _owner = seams::install_owner(script.hook());
    let (gate, probe, opened) = (ctx.cleanup.clone(), seed.probe.clone(), Cell::new(None));
    let (hook, findings) = observe(
        ctx.cleanup.clone(),
        Duration::from_secs(40),
        |_| (),
        move |runtime| {
            if opened.get().is_none() {
                opened.set(Some(probe.evidence().revision));
                gate.gates.borrow_mut().open("b");
            }
            let progress = runtime.reader.as_ref()?.view().snapshot();
            let done = exited(runtime) && progress.stdout.discarded > 0;
            done.then(|| (opened.get().unwrap(), probe.evidence().revision))
        },
    );
    let _retained = seams::install_owner_retained(hook);
    let (outcome, _) = with_case(&ctx, case, |case| drive_case(&mut case.runtime));
    assert_eq!(outcome, Outcome::Retained);
    let (before, after) = findings.take();
    assert_eq!(before, after, "storage revision unchanged");
    assert_eq!(seed.task_status().as_deref(), Some("running"));
    seed.assert_never_accepted();
}

type Accounts = (hf_platform::process::ReaderProgress, u64, u64, bool);
/// Per-stream accounting of a settlement after an overflow.
fn assert_loss_accounts((progress, out, err, closed): Accounts) {
    assert!(closed && progress.overflowed);
    assert_eq!(
        progress.stdout.read,
        progress.stdout.enqueued + progress.stdout.discarded
    );
    assert_eq!(
        progress.stderr.read,
        progress.stderr.enqueued + progress.stderr.discarded
    );
    assert_eq!(
        (out, err),
        (progress.stdout.enqueued, progress.stderr.enqueued)
    );
}
/// The private evidence of a known loss: observation flags, status and `loss.raw`.
fn assert_known_loss_evidence(seed: &super::harness::Seed) {
    let evidence = seed.evidence();
    assert!(evidence.observation.capture_known_loss && evidence.observation.observation_gap);
    assert!(evidence.observation.input_closed);
    assert_eq!(seed.task_status().as_deref(), Some("failed"));
    let b = &seed.binding;
    let loss = seed
        .root
        .join("private-payload")
        .join(b.generation.as_str())
        .join(b.task.as_str())
        .join(b.attempt.as_str())
        .join("loss.raw");
    assert_eq!(std::fs::metadata(loss).unwrap().len(), 19);
}

#[test]
fn reader_saturation_is_bounded_and_reported_incomplete() {
    let _serial = serial_case();
    let ctx = Ctx::new("saturation");
    let seed = ctx.seed(
        "read,init,gate:a,lines:3000,stderr:2000,gate:b,result,eofwait,exit:0",
        VERSION,
    );
    // The payload the agent emits is valid for the writer's stream.
    let mut stream = Stream::new(&seed.invitation).unwrap();
    let mut observation = ExecutionObservation::default();
    let mut bytes = init_line();
    (0..200).for_each(|i| bytes.extend(text_line(i)));
    stream.consume(&bytes, &mut observation).unwrap();
    assert!(!stream.malformed(), "{SESSION}");
    let gates = ctx.cleanup.clone();
    let pause = Act::Pause(Box::new(move |runtime| {
        gates.gates.borrow_mut().open("a");
        wait_for("reader overflow", Duration::from_secs(30), || {
            runtime
                .reader
                .as_ref()
                .unwrap()
                .view()
                .snapshot()
                .overflowed
        });
        gates.gates.borrow_mut().open("b");
    }));
    let script = Rc::new(Script::default()).on(OwnerPoint::BeforeStarted, pause);
    // The writer's stream must accept everything the agent emitted: record any malformation.
    let ever_malformed = Arc::new(AtomicBool::new(false));
    let flag = ever_malformed.clone();
    let config = WriterConfig {
        stream: Some(Arc::new(move |stream| {
            if stream.malformed() {
                flag.store(true, Ordering::Release);
            }
        })),
        ..WriterConfig::default()
    };
    let case = start_owner(&ctx, &seed, &config, &script, None);
    let _owner = seams::install_owner(script.hook());
    let (accounts, reply) = with_case(&ctx, case, |case| {
        let outcome = drive_case(&mut case.runtime);
        assert_eq!(outcome, Outcome::Settled(0));
        let runtime = &case.runtime;
        let progress = runtime.reader.as_ref().unwrap().view().snapshot();
        (
            progress,
            runtime.stdout_end,
            runtime.stderr_end,
            runtime.input_closed,
        )
    });
    started(reply);
    assert_loss_accounts(accounts);
    assert_known_loss_evidence(&seed);
    assert!(
        !ever_malformed.load(Ordering::Acquire),
        "the stream never went malformed"
    );
    seed.assert_never_accepted();
}
