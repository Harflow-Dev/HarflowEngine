use super::support::*;
use hf_application::prepared_launch::{CaptureContext, ExactCaptureReader};
use hf_domain::{
    fixture::{FixtureFault, Frame, ParentPlan},
    ownership::{Id, PreparedRequest},
    prepared_launch::CaptureAccepted,
    recovery::SourceStamp,
};
use hf_storage::{Limits, RecoveryReadSession, RootHold, Sha256Digest, Store};
use std::path::Path;

pub(super) fn assert_projection<'a>(
    reader: &ExactCaptureReader<
        RecoveryReadSession,
        hf_platform::recovery::ExactRawCaptureReader,
        Sha256Digest,
    >,
    invitation: &'a hf_domain::fixture::Invitation,
    request: &'a PreparedRequest,
    source: &'a hf_domain::recovery::SourceStamp,
    delivery: &Delivery,
    attempt: &Id,
) -> Vec<u8> {
    let captured = reader
        .read(
            CaptureContext {
                invitation,
                request,
                source,
            },
            delivery,
            deadline(3),
        )
        .unwrap();
    let bytes = captured.bytes.stdout;
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("fixed helper text")
    );
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("\"type\":\"result\"")
    );
    let mut decoder = hf_interfaces::claude_stream::ClaudeStreamDecoder::new(attempt.clone());
    for chunk in bytes.chunks(4096) {
        decoder.push(chunk);
    }
    let trace = decoder.finish();
    assert!(trace.ended && trace.fault.is_none());
    assert_eq!(
        trace.records.len(),
        3,
        "project only accepted captured JSONL bytes"
    );
    bytes
}
pub(super) struct ActualCapture {
    pub(super) request: PreparedRequest,
    pub(super) source: SourceStamp,
    pub(super) caller: hf_domain::process::ProcessIdentity,
    pub(super) receipt: CaptureAccepted,
    pub(super) reader: ExactCaptureReader<
        RecoveryReadSession,
        hf_platform::recovery::ExactRawCaptureReader,
        Sha256Digest,
    >,
}
fn create_store(
    scratch: &Path,
) -> (
    std::path::PathBuf,
    SourceStamp,
    hf_domain::fixture::QualifiedLedger,
) {
    let root = scratch.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        deadline(3),
        hf_platform::lease::shared(),
    )
    .unwrap();
    drop(store);
    let source = RootHold::open(&root, deadline(3), hf_platform::lease::shared())
        .unwrap()
        .source_stamp()
        .clone();
    (root, source, ledger(scratch))
}
pub(super) fn actual_capture(scratch: &Path) -> ActualCapture {
    let (root, source, ledger) = create_store(scratch);
    let mut child = spawn_parent(scratch);
    assert_eq!(
        child.initialization(),
        hf_platform::process::SpawnInitialization::Ready
    );
    let caller = child
        .registered_identity()
        .expect("retained parent identity")
        .clone();
    let pin = hf_platform::prepared_launch::observe_binary(&binary(), &Sha256Digest, deadline(3))
        .unwrap();
    let request = request(
        caller.clone(),
        &scratch.to_string_lossy(),
        pin,
        ledger,
        source.clone(),
    );
    send_plan(
        &mut child,
        ParentPlan {
            root: root.to_string_lossy().into_owned(),
            request: request.clone(),
            initialize: false,
            launch: true,
            owner: id('6'),
            nonce: id('7'),
            gate: None,
            ledger,
            fault: FixtureFault::None,
        },
    );
    let Frame::CaptureAccepted(receipt) = receive(&mut child) else {
        panic!("accepted source receipt required")
    };
    finish(&mut child);
    let reader = ExactCaptureReader::new(
        RecoveryReadSession::new(
            &root,
            scratch,
            Limits::default(),
            hf_platform::lease::shared(),
        ),
        hf_platform::recovery::ExactRawCaptureReader::new(&root),
        Sha256Digest,
    );
    ActualCapture {
        request,
        source,
        caller,
        receipt: *receipt,
        reader,
    }
}
