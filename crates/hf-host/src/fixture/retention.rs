//! Prepared-owner retention never signals, detaches, or substitutes PID authority.
use super::*;
use hf_domain::process::ProcessExit;

pub(super) fn settle(child: &mut RetainedOwnedChild) -> io::Result<()> {
    settle_observed(child, |_| {})
}
/// Observer runs after each finite poll on the original retained child handle.
/// A qualification deadline cannot cancel an admitted owner/storage operation.
pub(super) fn settle_observed(
    child: &mut RetainedOwnedChild,
    mut observe: impl FnMut(ProcessExit),
) -> io::Result<()> {
    child.close_stdin();
    let mut failed = false;
    loop {
        let until = Instant::now() + Duration::from_millis(20);
        failed |= child.pump(until, 4096).is_err();
        // Finite capture buffering is discarded only after the handoff completed
        // or became uncertain; these bytes can never manufacture a receipt.
        let _ = child.take_capture_bytes();
        let exit = match child.try_wait() {
            Ok(exit) => exit,
            Err(_) => {
                failed = true;
                ProcessExit::Unobserved
            }
        };
        observe(exit);
        if exit != ProcessExit::Unobserved {
            return if failed { Err(invalid()) } else { Ok(()) };
        }
        // Failed pipes can make polling immediate. Keep original wait authority
        // resident with a finite backoff; never invent a bounded shutdown claim.
        std::thread::sleep(Duration::from_millis(1));
    }
}
