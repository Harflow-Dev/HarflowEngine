#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub(crate) use super::port::{Call, FakePort, Fault};
pub(crate) use crate::owner::OwnerSession;
pub(crate) use hf_domain::{
    capture::{CaptureBinding, CaptureProgress, CaptureSnapshot, FileIdentity},
    ownership::{ClaimResult, Digest, Fence, Id, OwnerClaim, Phase, PreparedRequest, Snapshot},
    process::{ProcessExit, ProcessIdentity},
};
use std::{cell::RefCell, rc::Rc, time::Instant};

pub(crate) fn id(c: char) -> Id {
    Id::new(c.to_string().repeat(32)).unwrap()
}
pub(crate) fn digest(c: char) -> Digest {
    Digest::new(c.to_string().repeat(64)).unwrap()
}
pub(crate) fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(
        pid,
        100 + u64::from(pid),
        4,
        "abcdef01-2345-6789-abcd-ef0123456789".into(),
    )
    .unwrap()
}
pub(crate) fn request() -> PreparedRequest {
    PreparedRequest {
        operation: id('1'),
        session: id('2'),
        reservation: id('3'),
        attempt: id('4'),
        generation: id('5'),
        digest: digest('a'),
        payload: b"exact payload".to_vec(),
    }
}
pub(crate) fn owner() -> OwnerClaim {
    OwnerClaim {
        owner: id('6'),
        nonce: id('7'),
        process: process(41),
    }
}
pub(crate) fn prepared() -> Snapshot {
    Snapshot {
        request: request(),
        phase: Phase::Prepared,
        revision: 0,
        cancelled: false,
        owner: None,
        child: None,
    }
}
pub(crate) type Harness = (
    OwnerSession<FakePort>,
    Rc<RefCell<Vec<Call>>>,
    Rc<RefCell<bool>>,
);
pub(crate) fn harness(fault: Fault) -> Harness {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let dropped = Rc::new(RefCell::new(false));
    let port = FakePort {
        state: RefCell::new(prepared()),
        capture: RefCell::new(None),
        calls: calls.clone(),
        fault,
        dropped: dropped.clone(),
    };
    (
        OwnerSession::new(port, request(), owner(), 0),
        calls,
        dropped,
    )
}
pub(crate) fn deadline() -> Instant {
    Instant::now() + std::time::Duration::from_secs(1)
}
pub(crate) fn to_acceptance(session: &mut OwnerSession<FakePort>) {
    session.claim(deadline()).unwrap();
    session.record_child(&process(42), deadline()).unwrap();
    session.accept(deadline()).unwrap();
}
pub(crate) fn binding(session: &OwnerSession<FakePort>) -> CaptureBinding {
    let snapshot = session.snapshot().unwrap();
    CaptureBinding {
        fence: snapshot.fence().unwrap(),
        child: snapshot.child.clone().unwrap(),
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
