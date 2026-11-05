#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use super::process::deadline;
use hf_domain::process::ProcessExit;
use hf_platform::process::{
    MacIdentity, OwnedSignal, RetainedOwnedChild, SignalOutcome, WaitOutcome,
};

pub(super) fn cleanup(child: &mut RetainedOwnedChild) {
    child.close_stdin();
    match child.wait_until(deadline(2), || false) {
        Ok(WaitOutcome::Exited(_)) => return,
        Ok(WaitOutcome::DeadlineExpired) => {}
        Ok(WaitOutcome::Cancelled) => return uncertain("cleanup wait was cancelled"),
        Err(_) => return uncertain("cleanup wait failed"),
    }
    let Some(identity) = child.registered_identity().cloned() else {
        return uncertain("direct child identity is unknown");
    };
    match child.request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(1)) {
        Ok(SignalOutcome::Sent) => match child.wait_until(deadline(2), || false) {
            Ok(WaitOutcome::Exited(ProcessExit::Signal(9))) => {}
            Ok(WaitOutcome::Exited(_)) => uncertain("child exit did not confirm Kill"),
            Ok(WaitOutcome::DeadlineExpired) => uncertain("child remained unreaped after Kill"),
            Ok(WaitOutcome::Cancelled) => uncertain("post-Kill wait was cancelled"),
            Err(_) => uncertain("post-Kill wait failed"),
        },
        Ok(SignalOutcome::Refused(_)) => uncertain("qualified Kill was refused"),
        Ok(SignalOutcome::DeadlineExpired) => uncertain("qualified Kill missed its deadline"),
        Err(_) => uncertain("qualified Kill failed"),
    }
}

fn uncertain(reason: &str) {
    eprintln!("handoff refusal test cleanup uncertainty: {reason}");
    if !std::thread::panicking() {
        panic!("handoff refusal test cleanup uncertainty: {reason}");
    }
}
