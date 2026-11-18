#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_application::process::IdentityPort;
use hf_domain::{
    ownership::{Digest, Fence, Id, OwnerClaim},
    process::ProcessExit,
};
use hf_platform::{
    process::{CaptureOrigin, CaptureReader, MacIdentity, RetainedOwnedChild, SpawnSpec},
    task_runtime::settlement::OwnerSettlement,
};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};

fn id(digit: char) -> Id {
    Id::new(digit.to_string().repeat(32)).unwrap()
}
fn binding() -> Digest {
    Digest::new("a".repeat(64)).unwrap()
}
/// A settled child: its input is written and closed, it exited and its reader finished.
struct Settled {
    child: RetainedOwnedChild,
    reader: CaptureReader,
    origin: CaptureOrigin,
    out: u64,
    input: u64,
}
fn settle(script: &str, drain: bool) -> Settled {
    let spec = SpawnSpec {
        executable: "/bin/sh".into(),
        cwd: std::env::current_dir().unwrap(),
        args: ["-c", script].map(OsString::from).to_vec(),
        environment: Vec::new(),
        capture_limit: 0,
    };
    let end = Instant::now() + Duration::from_secs(20);
    let mut child = RetainedOwnedChild::spawn(spec, &MacIdentity, end).unwrap();
    let identity = child.registered_identity().cloned().unwrap();
    let owner = MacIdentity.identity(std::process::id()).unwrap();
    let origin = CaptureOrigin {
        child: identity,
        fence: Fence {
            operation: id('1'),
            generation: id('2'),
            attempt: id('3'),
            owner: OwnerClaim {
                owner: id('1'),
                nonce: id('4'),
                process: owner,
            },
            revision: 2,
        },
    };
    let mut reader = child.start_capture_reader(origin.clone(), None).unwrap();
    let input = b"hello\n";
    let mut written = 0;
    while written < input.len() {
        written += child.write_stdin(&input[written..], end).unwrap();
    }
    child.close_stdin();
    while Instant::now() < end {
        let exited = child.try_wait().unwrap() != ProcessExit::Unobserved;
        if exited && reader.is_finished() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    reader.join_finished().unwrap();
    let mut out = 0;
    if drain {
        while let Ok(batch) = reader.try_recv() {
            out += batch.stdout.len() as u64;
        }
    }
    Settled {
        child,
        reader,
        origin,
        out,
        input: input.len() as u64,
    }
}
const OVERFLOW: &str = "head -c 600000 /dev/zero; cat >/dev/null";

fn loss(s: &Settled, out: u64, input: u64) -> std::io::Result<OwnerSettlement> {
    OwnerSettlement::qualify_with_loss(&s.child, &s.reader, &s.origin, (out, 0, input), binding())
}

#[test]
fn overflow_settles_only_through_the_loss_qualification() {
    let s = settle(OVERFLOW, true);
    let progress = s.reader.view().snapshot();
    assert!(progress.overflowed && progress.discard && progress.stdout.discarded > 0);
    assert_eq!(
        progress.stdout.read,
        progress.stdout.enqueued + progress.stdout.discarded
    );
    assert_eq!(s.out, progress.stdout.enqueued);
    let ordinary = OwnerSettlement::qualify(
        &s.child,
        &s.reader,
        &s.origin,
        (s.out, 0, s.input),
        binding(),
    );
    assert!(
        ordinary.is_err(),
        "an overflow never qualifies without loss"
    );
    assert!(loss(&s, s.out, s.input).is_ok());
}

#[test]
fn each_preserved_constraint_still_refuses_with_loss() {
    let s = settle(OVERFLOW, true);
    assert!(loss(&s, s.out + 1, s.input).is_err(), "committed end ahead");
    assert!(
        loss(&s, s.out - 1, s.input).is_err(),
        "committed end behind"
    );
    assert!(
        loss(&s, u64::MAX, s.input).is_err(),
        "arithmetic is checked"
    );
    assert!(loss(&s, s.out, s.input + 1).is_err(), "input bytes differ");
    assert!(loss(&s, s.out, 0).is_err(), "no input");
    let other = CaptureOrigin {
        fence: Fence {
            revision: 9,
            ..s.origin.fence.clone()
        },
        ..s.origin.clone()
    };
    assert!(
        OwnerSettlement::qualify_with_loss(
            &s.child,
            &s.reader,
            &other,
            (s.out, 0, s.input),
            binding()
        )
        .is_err(),
        "origin differs"
    );
}

#[test]
fn a_requested_discard_or_a_queued_batch_never_qualifies_with_loss() {
    let s = settle(OVERFLOW, true);
    s.reader.discard_capture();
    assert!(loss(&s, s.out, s.input).is_err(), "requested discard");
    let undrained = settle(OVERFLOW, false);
    // Every other figure is real: only the queued batches can refuse.
    let enqueued = undrained.reader.view().snapshot().stdout.enqueued;
    assert!(enqueued > 0);
    assert!(
        loss(&undrained, enqueued, undrained.input).is_err(),
        "queued batches remain"
    );
}

#[test]
fn loss_qualification_requires_an_actual_overflow() {
    let s = settle("printf ok; cat >/dev/null", true);
    assert!(!s.reader.view().snapshot().overflowed);
    assert!(loss(&s, s.out, s.input).is_err());
    assert!(
        OwnerSettlement::qualify(
            &s.child,
            &s.reader,
            &s.origin,
            (s.out, 0, s.input),
            binding()
        )
        .is_ok()
    );
}
