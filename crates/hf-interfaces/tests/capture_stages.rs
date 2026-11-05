#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use hf_domain::{
    fixture::{Frame, Gate, ParentPlan, QualifiedLedger, Stage},
    ownership::{Digest, Id, PreparedRequest},
};
use hf_interfaces::fixture::{decode, encode};

fn id(byte: char) -> Id {
    Id::new(byte.to_string().repeat(32)).unwrap()
}
fn parent(stage: Stage) -> Frame {
    let digest = Digest::new("a".repeat(64)).unwrap();
    Frame::Parent(ParentPlan {
        root: "/private/tmp/capture/store".into(),
        request: PreparedRequest {
            operation: id('1'),
            session: id('2'),
            reservation: id('3'),
            attempt: id('4'),
            generation: id('5'),
            digest,
            payload: Vec::new(),
        },
        initialize: true,
        launch: true,
        owner: id('6'),
        nonce: id('7'),
        gate: Some(Gate {
            stage,
            path: "/private/tmp/capture/gate.sock".into(),
        }),
        ledger: QualifiedLedger {
            device: 11,
            inode: 12,
        },
        fault: hf_domain::fixture::FixtureFault::None,
    })
}

#[test]
fn every_capture_ordering_stage_round_trips_through_the_private_frame_codec() {
    for stage in [
        Stage::CaptureBeforeSync,
        Stage::CaptureAfterSync,
        Stage::CaptureAfterPublish,
        Stage::CaptureBeforeTerminal,
        Stage::CaptureAfterTerminal,
    ] {
        let frame = parent(stage);
        let bytes = encode(&frame).unwrap();
        assert_eq!(decode(&bytes).unwrap(), frame, "stage {stage:?}");
    }
}
