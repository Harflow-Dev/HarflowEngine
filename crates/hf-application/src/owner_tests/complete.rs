use super::support::*;
use crate::owner::OwnerError;

#[test]
fn complete_lossless_capture_finishes_once_and_every_later_mutation_is_refused() {
    let (mut session, calls, _) = harness(Fault::None);
    to_acceptance(&mut session);
    let bind = binding(&session);
    session.begin_capture(&bind, deadline()).unwrap();
    for (revision, stdout, stderr, out_eof, err_eof) in
        [(1, 3, 2, false, false), (2, 3, 2, true, true)]
    {
        session
            .publish_capture(
                &CaptureProgress {
                    revision,
                    stdout,
                    stderr,
                    stdout_eof: out_eof,
                    stderr_eof: err_eof,
                    loss: false,
                },
                deadline(),
            )
            .unwrap();
    }
    session
        .finish_capture(ProcessExit::Code(0), deadline())
        .unwrap();
    assert_eq!(session.capture().unwrap().exit, ProcessExit::Code(0));
    assert_eq!(
        session.finish_capture(ProcessExit::Code(0), deadline()),
        Err(OwnerError::InvalidState)
    );
    assert!(session.is_poisoned());
    assert_eq!(
        &*calls.borrow(),
        &[
            Call::Observe,
            Call::Claim,
            Call::Spawn,
            Call::Accept,
            Call::Begin,
            Call::Publish,
            Call::Publish,
            Call::Finish
        ]
    );
}
