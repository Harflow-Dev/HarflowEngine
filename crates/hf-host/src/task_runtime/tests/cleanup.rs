//! Cleanup of fake agents, descendants and owners.
use super::agents::Gates;
use crate::task_runtime::{owner_runtime::Runtime, writer_slot::WriterSlot};
use hf_application::process::IdentityPort as _;
use hf_domain::process::ProcessIdentity;
use hf_platform::process::MacIdentity;
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

/// A running descendant of a fake agent, by identity.
pub(super) struct Cleanup {
    pub(super) gates: RefCell<Gates>,
    pub(super) descendants: RefCell<Vec<ProcessIdentity>>,
}
impl Cleanup {
    pub(super) fn register(&self, identity: ProcessIdentity) {
        self.descendants.borrow_mut().push(identity);
    }
    /// Ends every registered descendant: first cooperatively by closing the gates, then,
    /// only while the recorded identity still matches, with `/bin/kill -KILL` (a bounded
    /// fallback: the pid could in principle be reused between the check and the signal).
    pub(super) fn end_descendants(&self) {
        self.gates.borrow_mut().close_all();
        for identity in self.descendants.borrow().iter() {
            if !gone(identity, Duration::from_secs(1)) {
                if MacIdentity.identity(identity.pid()).as_ref() == Some(identity) {
                    let _ = std::process::Command::new("/bin/kill")
                        .args(["-KILL", &identity.pid().to_string()])
                        .status();
                }
                if !gone(identity, Duration::from_secs(2)) {
                    eprintln!("descendant {} did not end", identity.pid());
                }
            }
        }
        self.descendants.borrow_mut().clear();
    }
    /// The owner's resources, ended in the order the unwind protocol requires.
    pub(super) fn owner(&self, runtime: &mut Runtime) {
        kill_agent(runtime);
        self.end_descendants();
        if let Some(reader) = &runtime.reader {
            let end = Instant::now() + Duration::from_secs(3);
            while !reader.is_finished() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(2));
            }
            if !reader.is_finished() {
                eprintln!("capture reader could not finish; aborting");
                std::process::abort();
            }
        }
        if let Some(WriterSlot::Thread(thread)) = &mut runtime.writer {
            thread.release_and_join(Duration::from_secs(5));
        }
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.end_descendants();
    }
}
fn gone(identity: &ProcessIdentity, within: Duration) -> bool {
    let end = Instant::now() + within;
    loop {
        if MacIdentity.identity(identity.pid()).as_ref() != Some(identity) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn kill_agent(runtime: &mut Runtime) {
    let Some(agent) = runtime.agent.as_mut() else {
        return;
    };
    let end = Instant::now() + Duration::from_secs(2);
    if agent.try_wait().unwrap() == hf_domain::process::ProcessExit::Unobserved {
        if let Some(identity) = agent.registered_identity().cloned() {
            let _ = agent.request_signal(
                &identity,
                &MacIdentity,
                hf_platform::process::OwnedSignal::Kill,
                end,
            );
        }
        while agent.try_wait().unwrap() == hf_domain::process::ProcessExit::Unobserved
            && Instant::now() < end
        {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
