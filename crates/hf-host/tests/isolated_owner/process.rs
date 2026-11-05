use crate::evidence::{
    read_writer_identity, wait_root_available, wait_root_available_result, wait_writer_dead,
    wait_writer_dead_or_abort,
};
use crate::support::{self, Scratch, built_scaffold, deadline};
use hf_application::process::IdentityPort;
use hf_domain::{
    fixture::{Frame, Gate, Invitation},
    process::ProcessIdentity,
};
use hf_platform::process::{MacIdentity, OwnedSignal, RetainedOwnedChild, SpawnSpec, WaitOutcome};
use std::{ffi::OsString, time::Instant};

pub(super) struct CurrentDirectory(std::path::PathBuf);
impl CurrentDirectory {
    pub(super) fn enter(path: &std::path::Path) -> Self {
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self(old)
    }
}
impl Drop for CurrentDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).expect("restore test process cwd");
    }
}

pub(super) struct OwnerGuard {
    child: Option<RetainedOwnedChild>,
    root: std::path::PathBuf,
    pub(super) writer: Option<ProcessIdentity>,
    pub(super) agent: Option<ProcessIdentity>,
    reaped: bool,
    settled: bool,
}
impl OwnerGuard {
    pub(super) fn spawn(scratch: &Scratch) -> Self {
        let child = RetainedOwnedChild::spawn(
            SpawnSpec {
                executable: built_scaffold(),
                cwd: scratch.path.clone(),
                args: vec!["--internal-isolated-owner-fixture-v1".into()],
                environment: Vec::<(OsString, OsString)>::new(),
                capture_limit: 32768,
            },
            &MacIdentity,
            deadline(3),
        )
        .expect("spawn sourcefixed owner");
        Self {
            child: Some(child),
            root: scratch.root.clone(),
            writer: None,
            agent: None,
            reaped: false,
            settled: false,
        }
    }

    fn child(&mut self) -> &mut RetainedOwnedChild {
        self.child.as_mut().expect("owner handle")
    }

    pub(super) fn handshake(&mut self, scratch: &Scratch, gate: Gate) {
        let child = self.child();
        assert_eq!(
            child.initialization(),
            hf_platform::process::SpawnInitialization::Ready
        );
        let process = child
            .registered_identity()
            .expect("original owner identity")
            .clone();
        assert_eq!(decode(child, deadline(2)), Frame::Hello(process.clone()));
        let invitation = Invitation {
            operation: scratch.request.operation.clone(),
            generation: scratch.request.generation.clone(),
            digest: scratch.request.digest.clone(),
            owner: support::id(support::OWNER),
            nonce: support::id(support::NONCE),
            process,
            revision: 0,
        };
        send(child, &Frame::Invitation(invitation.clone()), deadline(2));
        assert_eq!(decode(child, deadline(2)), Frame::Ready(invitation.clone()));
        send(
            child,
            &Frame::Prepared(scratch.prepared(invitation.process.clone(), gate)),
            deadline(2),
        );
        child.close_stdin();
    }

    pub(super) fn kill_and_reap(&mut self) {
        let participant = self
            .root
            .join(format!("owner-participant-{}.v1", support::OP));
        self.writer = Some(read_writer_identity(&participant));
        let child = self.child();
        let identity = child
            .registered_identity()
            .expect("registered original owner")
            .clone();
        assert_eq!(
            MacIdentity.identity(identity.pid()).as_ref(),
            Some(&identity),
            "fresh exact owner identity"
        );
        let until = deadline(3);
        let signal = child
            .request_signal(&identity, &MacIdentity, OwnedSignal::Kill, until)
            .expect("signal retained owner");
        assert!(
            matches!(signal, hf_platform::process::SignalOutcome::Sent),
            "original owner kill must be sent: {signal:?}"
        );
        assert!(
            matches!(
                child.wait_until(until, || false),
                Ok(WaitOutcome::Exited(_))
            ),
            "original owner must be reaped"
        );
        self.reaped = true;
    }

    pub(super) fn finish_writer(&mut self, until: Instant) {
        if let Some(writer) = &self.writer {
            wait_writer_dead(writer, until);
        }
        if let Some(agent) = &self.agent {
            wait_writer_dead(agent, until);
        }
        wait_root_available(&self.root, until);
        self.settled = true;
    }
}
impl Drop for OwnerGuard {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let Some(child) = self.child.as_mut() else {
            std::process::abort();
        };
        if !self.reaped {
            // GatePeer drops first and releases the writer. Give the fixed
            // owner/writer their complete cooperative budgets before escalation.
            if !matches!(
                child.wait_until(deadline(12), || false),
                Ok(WaitOutcome::Exited(_))
            ) {
                // Without sidecar evidence, killing the owner could orphan a
                // bootstrapping writer we cannot positively observe afterward.
                if self.writer.is_none() {
                    std::process::abort();
                }
                let Some(identity) = child.registered_identity().cloned() else {
                    std::process::abort();
                };
                if MacIdentity.identity(identity.pid()).as_ref() != Some(&identity) {
                    std::process::abort();
                }
                match child.request_signal(&identity, &MacIdentity, OwnedSignal::Kill, deadline(2))
                {
                    Ok(hf_platform::process::SignalOutcome::Sent) => (),
                    _ => std::process::abort(),
                }
                if !matches!(
                    child.wait_until(deadline(2), || false),
                    Ok(WaitOutcome::Exited(_))
                ) {
                    std::process::abort();
                }
            }
            self.reaped = true;
        }
        let until = deadline(8);
        if let Some(writer) = &self.writer {
            wait_writer_dead_or_abort(writer, until);
        }
        if let Some(agent) = &self.agent {
            wait_writer_dead_or_abort(agent, until);
        }
        if wait_root_available_result(&self.root, until).is_err() {
            std::process::abort();
        }
        self.settled = true;
    }
}

fn decode(child: &RetainedOwnedChild, until: Instant) -> Frame {
    hf_interfaces::fixture::decode(
        &child
            .read_fixture_frame(until)
            .expect("read owner HFB2 frame"),
    )
    .expect("decode owner HFB2 frame")
}
fn send(child: &RetainedOwnedChild, frame: &Frame, until: Instant) {
    child
        .write_fixture_frame(
            &hf_interfaces::fixture::encode(frame).expect("encode HFB2 frame"),
            until,
        )
        .expect("write owner HFB2 frame");
}
