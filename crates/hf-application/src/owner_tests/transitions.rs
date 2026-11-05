use super::support::*;
use crate::owner::{OwnerError, OwnerSession};

#[test]
fn child_acceptance_and_capture_require_exact_phase_fence_child_and_complete_eof() {
    let (mut session, calls, _) = harness(Fault::None);
    assert_eq!(
        session.begin_capture(&dummy_binding(), deadline()),
        Err(OwnerError::InvalidState)
    );
    assert!(session.is_poisoned());
    assert_eq!(&*calls.borrow(), &[]);

    let (mut session, calls, _) = harness(Fault::None);
    to_acceptance(&mut session);
    let mut capture = binding(&session);
    let mut wrong = capture.clone();
    wrong.fence.revision += 1;
    assert_eq!(
        session.begin_capture(&wrong, deadline()),
        Err(OwnerError::InvalidState)
    );
    assert_eq!(calls.borrow().last(), Some(&Call::Accept));
    assert!(session.is_poisoned());

    let (mut session, _, _) = harness(Fault::None);
    to_acceptance(&mut session);
    capture = binding(&session);
    session.begin_capture(&capture, deadline()).unwrap();
    assert_eq!(
        session.finish_capture(ProcessExit::Code(0), deadline()),
        Err(OwnerError::InvalidState)
    );
    assert!(session.is_poisoned());
}

#[test]
fn invalid_child_loss_and_capture_cap_inputs_poison_before_any_further_write() {
    let (mut same_owner, calls, _) = harness(Fault::None);
    same_owner.claim(deadline()).unwrap();
    assert_eq!(
        same_owner.record_child(&owner().process, deadline()),
        Err(OwnerError::InvalidState)
    );
    assert_eq!(calls.borrow().last(), Some(&Call::Claim));

    let (mut lost, calls, _) = harness(Fault::None);
    to_acceptance(&mut lost);
    let bind = binding(&lost);
    lost.begin_capture(&bind, deadline()).unwrap();
    lost.publish_capture(
        &CaptureProgress {
            revision: 1,
            stdout: 1,
            stderr: 0,
            stdout_eof: false,
            stderr_eof: false,
            loss: true,
        },
        deadline(),
    )
    .unwrap();
    assert_eq!(
        lost.finish_capture(ProcessExit::Code(0), deadline()),
        Err(OwnerError::InvalidState)
    );
    assert_eq!(calls.borrow().last(), Some(&Call::Publish));
    assert!(lost.is_poisoned());

    let (mut capped, calls, _) = harness(Fault::None);
    to_acceptance(&mut capped);
    let bind = binding(&capped);
    capped.begin_capture(&bind, deadline()).unwrap();
    assert_eq!(
        capped.publish_capture(
            &CaptureProgress {
                revision: 1,
                stdout: 65_537,
                stderr: 0,
                stdout_eof: false,
                stderr_eof: false,
                loss: false,
            },
            deadline(),
        ),
        Err(OwnerError::InvalidState)
    );
    assert_eq!(calls.borrow().last(), Some(&Call::Begin));
    assert!(capped.is_poisoned());
}

fn dummy_binding() -> CaptureBinding {
    CaptureBinding {
        fence: Fence {
            operation: id('1'),
            generation: id('5'),
            attempt: id('4'),
            owner: owner(),
            revision: 1,
        },
        child: process(42),
        stdout: FileIdentity {
            device: 1,
            inode: 2,
        },
        stderr: FileIdentity {
            device: 1,
            inode: 3,
        },
    }
}

fn fail_at(session: &mut OwnerSession<FakePort>, target: Call) {
    if target != Call::Claim {
        session.claim(deadline()).unwrap();
    }
    if matches!(
        target,
        Call::Accept | Call::Begin | Call::Publish | Call::Finish
    ) {
        session.record_child(&process(42), deadline()).unwrap();
    }
    if matches!(target, Call::Begin | Call::Publish | Call::Finish) {
        session.accept(deadline()).unwrap();
    }
    let bind = if matches!(target, Call::Begin | Call::Publish | Call::Finish) {
        Some(binding(session))
    } else {
        None
    };
    if matches!(target, Call::Publish | Call::Finish) {
        session
            .begin_capture(bind.as_ref().unwrap(), deadline())
            .unwrap();
    }
    if target == Call::Finish {
        session
            .publish_capture(
                &CaptureProgress {
                    revision: 1,
                    stdout: 0,
                    stderr: 0,
                    stdout_eof: true,
                    stderr_eof: true,
                    loss: false,
                },
                deadline(),
            )
            .unwrap();
    }
    match target {
        Call::Claim => {
            let _ = session.claim(deadline());
        }
        Call::Spawn => {
            let _ = session.record_child(&process(42), deadline());
        }
        Call::Accept => {
            let _ = session.accept(deadline());
        }
        Call::Begin => {
            let _ = session.begin_capture(bind.as_ref().unwrap(), deadline());
        }
        Call::Publish => {
            let _ = session.publish_capture(
                &CaptureProgress {
                    revision: 1,
                    stdout: 1,
                    stderr: 0,
                    stdout_eof: false,
                    stderr_eof: false,
                    loss: false,
                },
                deadline(),
            );
        }
        Call::Finish => {
            let _ = session.finish_capture(ProcessExit::Code(0), deadline());
        }
        _ => unreachable!(),
    }
}

#[test]
fn every_ambiguous_error_or_malformed_write_permanently_blocks_port_mutation() {
    for target in [
        Call::Claim,
        Call::Spawn,
        Call::Accept,
        Call::Begin,
        Call::Publish,
        Call::Finish,
    ] {
        for malformed in [false, true] {
            let fault = if malformed {
                Fault::Malformed(target)
            } else {
                Fault::Error(target)
            };
            let (mut session, calls, dropped) = harness(fault);
            fail_at(&mut session, target);
            assert!(
                session.is_poisoned(),
                "target={target:?} malformed={malformed}"
            );
            assert!(!*dropped.borrow(), "poisoned session retains its port");
            let after_failure = calls.borrow().len();
            assert_eq!(session.claim(deadline()), Err(OwnerError::Poisoned));
            assert_eq!(
                session.record_child(&process(43), deadline()),
                Err(OwnerError::Poisoned)
            );
            assert_eq!(session.accept(deadline()), Err(OwnerError::Poisoned));
            assert_eq!(
                session.begin_capture(&dummy_binding(), deadline()),
                Err(OwnerError::Poisoned)
            );
            assert_eq!(
                session.publish_capture(
                    &CaptureProgress {
                        revision: 1,
                        stdout: 1,
                        stderr: 0,
                        stdout_eof: false,
                        stderr_eof: false,
                        loss: false
                    },
                    deadline()
                ),
                Err(OwnerError::Poisoned)
            );
            assert_eq!(
                session.finish_capture(ProcessExit::Code(0), deadline()),
                Err(OwnerError::Poisoned)
            );
            assert_eq!(
                calls.borrow().len(),
                after_failure,
                "no later port call after {target:?}"
            );
        }
    }
}
