//! Fast agents lose nothing: an unbuffered burst and a megabyte burst, and the two ways a
//! stalled capture still ends in known loss.
use super::{
    ack_loss::retain_with,
    harness::{Ctx, Outcome, Script, Seed, WriterConfig, drive_case, start_owner, with_case},
    owner_cases::started,
    serial_case,
};
use crate::task_runtime::seams;
use std::{rc::Rc, time::Duration};

const VERSION: &str = hf_backends::backend_launch::CLAUDE_VERSION;

pub(super) fn raw_file(seed: &Seed, name: &str) -> std::path::PathBuf {
    let b = &seed.binding;
    seed.root
        .join("private-payload")
        .join(b.generation.as_str())
        .join(b.task.as_str())
        .join(b.attempt.as_str())
        .join(name)
}
/// Nothing was lost: `loss.raw` is empty, the flag is false, and both raw files hold exactly
/// the bytes the agent wrote, ending at the acknowledged offsets.
fn assert_lossless(ctx: &Ctx, seed: &Seed) {
    assert_eq!(std::fs::read(raw_file(seed, "loss.raw")).unwrap(), b"");
    let evidence = seed.evidence();
    assert!(!evidence.observation.capture_known_loss);
    let (out, err) = ctx.fake.emitted();
    assert!(!out.is_empty());
    assert_eq!(std::fs::read(raw_file(seed, "stdout.raw")).unwrap(), out);
    assert_eq!(std::fs::read(raw_file(seed, "stderr.raw")).unwrap(), err);
    assert_eq!(evidence.observation.stdout_end, out.len() as u64);
    assert_eq!(evidence.observation.stderr_end, err.len() as u64);
    assert_eq!(seed.task_status().as_deref(), Some("completed"));
    seed.assert_never_accepted();
}
fn lossless_run(name: &str, steps: &str, config: &WriterConfig) {
    let _serial = serial_case();
    let ctx = Ctx::new(name);
    let seed = ctx.seed(steps, VERSION);
    let script = Rc::new(Script::default());
    let case = start_owner(&ctx, &seed, config, &script, None);
    let (outcome, reply) = with_case(&ctx, case, |case| drive_case(&mut case.runtime));
    assert_eq!(outcome, Outcome::Settled(0));
    started(reply);
    assert_lossless(&ctx, &seed);
}

#[test]
fn unbuffered_burst_completes_without_loss() {
    let config = WriterConfig {
        exchange_delay: Some(Duration::from_millis(10)),
        ..WriterConfig::default()
    };
    lossless_run(
        "burst",
        "read,init,burst:12,stderr:300,text,result,eofwait,exit:0",
        &config,
    );
}

#[test]
fn megabyte_burst_with_slowed_writer_loses_nothing() {
    let config = WriterConfig {
        exchange_delay: Some(Duration::from_millis(15)),
        ..WriterConfig::default()
    };
    lossless_run("mib", "read,init,mib:1,result,eofwait,exit:0", &config);
}

/// A prompt larger than the agent's stdin pipe, so the owner cannot finish writing it
/// while the agent is not reading.
fn large_prompt() -> String {
    "p".repeat(300_000)
}

#[test]
fn spawn_window_cross_block_reaches_known_loss_and_settles() {
    let _serial = serial_case();
    let ctx = Ctx::new("cross-block");
    // The agent floods stdout before it reads its prompt, while the owner is still writing
    // that prompt and dequeues nothing: only the stall releases both.
    let seed = ctx.seed_prompt(
        "flood:2000000,read,init,text,result,eofwait,exit:0",
        VERSION,
        &large_prompt(),
    );
    let _stall = seams::install_stall(Rc::new(|| Some(Duration::from_millis(300))));
    let script = Rc::new(Script::default());
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let (outcome, reply) = with_case(&ctx, case, |case| drive_case(&mut case.runtime));
    assert_eq!(outcome, Outcome::Settled(0));
    started(reply);
    assert_eq!(
        std::fs::read(raw_file(&seed, "loss.raw")).unwrap(),
        b"capture-known-loss stall\n"
    );
    let evidence = seed.evidence();
    assert!(evidence.observation.capture_known_loss && evidence.observation.observation_gap);
    assert_eq!(seed.task_status().as_deref(), Some("failed"));
    seed.assert_never_accepted();
}

#[test]
fn child_that_never_reads_input_keeps_unknown_retention() {
    let _serial = serial_case();
    let ctx = Ctx::new("never-reads");
    let seed = ctx.seed_prompt("noread", VERSION, &large_prompt());
    let script = Rc::new(Script::default());
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let (found, _reply) = retain_with(&ctx, &seed, &script, case);
    assert!(
        found.agent && found.reader,
        "the agent and its reader are still held"
    );
    assert_eq!(seed.task_status().as_deref(), Some("running"));
    let cause = std::fs::read_to_string(raw_file(&seed, "owner-retained.cause")).unwrap();
    assert!(
        cause.contains("phase: start\n") && cause.contains("kind: poisoned\n"),
        "{cause}"
    );
    seed.assert_never_accepted();
}
