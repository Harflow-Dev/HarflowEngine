use hf_domain::{
    fixture::{
        EffectStart, FixtureFault, Frame, Gate, Invitation, ParentPlan, Prepared, QualifiedLedger,
        ReportKind, Stage,
    },
    ownership::{Digest, Id, OwnerClaim, PreparedRequest},
    process::ProcessIdentity,
};

fn id(byte: char) -> Id {
    Id::new(byte.to_string().repeat(32)).unwrap()
}

fn digest(byte: char) -> Digest {
    Digest::new(byte.to_string().repeat(64)).unwrap()
}

fn process(pid: u32, start: u64) -> ProcessIdentity {
    ProcessIdentity::new(
        pid,
        start,
        17,
        "abcdef01-2345-6789-abcd-ef0123456789".into(),
    )
    .unwrap()
}

fn invitation() -> Invitation {
    Invitation {
        operation: id('1'),
        generation: id('2'),
        digest: digest('a'),
        owner: id('3'),
        nonce: id('4'),
        process: process(42, 99),
        revision: 8,
    }
}

fn request() -> PreparedRequest {
    PreparedRequest {
        operation: id('1'),
        session: id('5'),
        reservation: id('6'),
        attempt: id('7'),
        generation: id('2'),
        digest: digest('a'),
        payload: vec![0, 1, 2, 255],
    }
}

fn frames() -> Vec<Frame> {
    let invite = invitation();
    let owner = OwnerClaim {
        owner: invite.owner.clone(),
        nonce: invite.nonce.clone(),
        process: invite.process.clone(),
    };
    let fence = hf_domain::ownership::Fence {
        operation: invite.operation.clone(),
        generation: invite.generation.clone(),
        attempt: request().attempt,
        owner,
        revision: invite.revision + 1,
    };
    let ledger = QualifiedLedger {
        device: 11,
        inode: 12,
    };
    vec![
        Frame::Hello(process(43, 100)),
        Frame::Invitation(invite.clone()),
        Frame::Ready(invite.clone()),
        Frame::Prepared(Prepared {
            invitation: invite.clone(),
            request: request(),
            root: "/private/tmp/fixture-root".into(),
            gate: Some(Gate {
                stage: Stage::OwnerBeforeAck,
                path: "/private/tmp/gate.sock".into(),
            }),
            ledger,
            fault: FixtureFault::None,
        }),
        Frame::Accepted {
            invitation: invite.clone(),
            fence: fence.clone(),
            child: process(44, 101),
        },
        Frame::Parent(ParentPlan {
            root: "/private/tmp/fixture-root".into(),
            request: request(),
            initialize: false,
            launch: true,
            owner: invite.owner.clone(),
            nonce: invite.nonce.clone(),
            gate: None,
            ledger,
            fault: FixtureFault::None,
        }),
        Frame::Start(EffectStart {
            operation: invite.operation.clone(),
            root: "/private/tmp/fixture-root".into(),
            ledger,
            fence,
        }),
        Frame::EffectDone {
            operation: invite.operation.clone(),
            process: process(45, 102),
        },
        Frame::Report {
            operation: invite.operation,
            generation: id('2'),
            kind: ReportKind::Uncertain,
        },
    ]
}

#[test]
fn each_private_frame_round_trips_with_complete_binding_fields() {
    for frame in frames() {
        let bytes = super::fixture::encode(&frame).unwrap();
        assert!(bytes.len() <= 24_576);
        assert_eq!(super::fixture::decode(&bytes).unwrap(), frame);
    }
}

#[test]
fn effect_start_round_trip_preserves_the_complete_storage_fence() {
    let frame = frames()[6].clone();
    let Frame::Start(start) = &frame else {
        panic!("effect start fixture missing");
    };
    assert_eq!(start.fence.owner.process, process(42, 99));
    assert_eq!(start.fence.operation, start.operation);
    assert_eq!(
        super::fixture::decode(&super::fixture::encode(&frame).unwrap()).unwrap(),
        frame
    );
}

#[test]
fn decoder_rejects_incomplete_extra_and_oversized_frames_before_acceptance() {
    let bytes = super::fixture::encode(&frames()[3]).unwrap();
    for end in [0, 1, 3, 4, bytes.len() - 1] {
        assert!(super::fixture::decode(&bytes[..end]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(super::fixture::decode(&trailing).is_err());
    let too_large = vec![0xff; 24_577];
    assert!(super::fixture::decode(&too_large).is_err());
}

#[test]
fn decoder_rejects_bad_prefix_magic_version_tag_and_declared_lengths() {
    let original = super::fixture::encode(&frames()[0]).unwrap();
    for (index, value) in [(3, 0), (4, b'X'), (8, 2), (9, 99)] {
        let mut malformed = original.clone();
        malformed[index] = value;
        assert!(super::fixture::decode(&malformed).is_err());
    }
    let mut short_declared = original.clone();
    short_declared[3] = short_declared[3].saturating_sub(1);
    assert!(super::fixture::decode(&short_declared).is_err());
    let mut long_declared = original;
    long_declared[3] = long_declared[3].saturating_add(1);
    assert!(super::fixture::decode(&long_declared).is_err());

    let mut invalid_identity = super::fixture::encode(&frames()[0]).unwrap();
    invalid_identity[30] = b'!';
    assert!(super::fixture::decode(&invalid_identity).is_err());

    let mut invalid_id_case = super::fixture::encode(&frames()[1]).unwrap();
    invalid_id_case[10] = b'A';
    assert!(super::fixture::decode(&invalid_id_case).is_err());
}

#[test]
fn encoder_enforces_the_total_frame_limit() {
    let mut frame = frames()[3].clone();
    if let Frame::Prepared(prepared) = &mut frame {
        prepared.request.payload = vec![0x5a; 20_000];
    }
    assert!(super::fixture::encode(&frame).is_err());
}

#[test]
fn parent_effect_and_report_frames_round_trip_typed_private_controls() {
    for frame in frames().into_iter().skip(5) {
        let bytes = super::fixture::encode(&frame).unwrap();
        assert_eq!(super::fixture::decode(&bytes).unwrap(), frame);
    }
    for kind in [
        ReportKind::Prepared,
        ReportKind::Existing,
        ReportKind::Refused,
        ReportKind::Uncertain,
    ] {
        let report = Frame::Report {
            operation: id('1'),
            generation: id('2'),
            kind,
        };
        let bytes = super::fixture::encode(&report).unwrap();
        assert_eq!(super::fixture::decode(&bytes).unwrap(), report);
    }
}

#[test]
fn parent_frame_rejects_non_boolean_control_bytes() {
    let Frame::Parent(parent) = frames()[5].clone() else {
        return;
    };
    let mut bytes = super::fixture::encode(&Frame::Parent(parent.clone())).unwrap();
    let bool_at = 10 + 4 + parent.root.len() + 5 * 32 + 64 + 4 + parent.request.payload.len();
    bytes[bool_at] = 2;
    assert!(super::fixture::decode(&bytes).is_err());
}

#[test]
fn parent_and_prepared_frames_round_trip_deny_only_faults_and_reject_unknown_values() {
    for fault in [
        FixtureFault::None,
        FixtureFault::UnknownChildIdentity,
        FixtureFault::UnusableChildPipe,
    ] {
        let mut prepared = frames()[3].clone();
        let Frame::Prepared(value) = &mut prepared else {
            panic!("prepared frame fixture missing")
        };
        value.fault = fault;
        let encoded = super::fixture::encode(&prepared).unwrap();
        assert_eq!(super::fixture::decode(&encoded).unwrap(), prepared);
        let mut parent = frames()[5].clone();
        let Frame::Parent(value) = &mut parent else {
            panic!("parent frame fixture missing")
        };
        value.fault = fault;
        let encoded = super::fixture::encode(&parent).unwrap();
        assert_eq!(super::fixture::decode(&encoded).unwrap(), parent);
    }
    for index in [3, 5] {
        let mut encoded = super::fixture::encode(&frames()[index]).unwrap();
        let last = encoded.len() - 1;
        encoded[last] = 3;
        assert!(super::fixture::decode(&encoded).is_err());
    }
}

#[test]
fn prepared_frame_rejects_noncanonical_roots_gate_paths_and_oversized_root() {
    for root in ["relative", "/", "/a/../b", "/a//b", "/a/./b", "/a/"] {
        let mut frame = frames()[3].clone();
        let Some(prepared) = prepared_mut(&mut frame) else {
            return;
        };
        prepared.root = root.into();
        assert!(super::fixture::encode(&frame).is_err());
    }
    let mut frame = frames()[3].clone();
    if let Some(prepared) = prepared_mut(&mut frame) {
        prepared.root = format!("/{}", "x".repeat(4096));
    }
    assert!(super::fixture::encode(&frame).is_err());
    if let Some(prepared) = prepared_mut(&mut frame) {
        prepared.root = "/private/tmp/root".into();
        prepared.gate.as_mut().unwrap().path = "/private/tmp/../outside.sock".into();
    }
    assert!(super::fixture::encode(&frame).is_err());
}

fn prepared_mut(frame: &mut Frame) -> Option<&mut Prepared> {
    match frame {
        Frame::Prepared(value) => Some(value),
        _ => None,
    }
}
