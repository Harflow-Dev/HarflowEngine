#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;

#[path = "pipeline/support.rs"]
mod support;
pub(super) use support::Observation;
use support::{ParentGuard, bounded, identity_summary, show};

#[path = "pipeline/prepare.rs"]
mod prepare;
use prepare::{
    OwnerArtifacts, build_qualification, build_request, parent_identity, prepare_artifacts,
    spawn_parent,
};

pub(super) fn run_owner_pipeline(
    measurement: &mut Measurement,
    overall: Instant,
    initial_vendor: &hf_domain::prepared_launch::BinaryPin,
) -> Result<Observation, String> {
    let artifacts = prepare_artifacts(measurement, overall, initial_vendor)?;
    let _cwd = CurrentDirectory::enter(&artifacts.root);
    let parent_started = Instant::now();
    let nonce = measurement.nonce.clone();
    let mut parent = spawn_parent(measurement, &artifacts, overall)?;
    let caller = parent_identity(&mut parent)?;
    let qualification = build_qualification(&artifacts, caller, nonce)?;
    let request = build_request(&qualification)?;
    send_parent_plan(&mut parent, &artifacts, &request, overall)?;
    let child = observe_held_post(&artifacts, &qualification, &request, overall)?;
    parent.release_post()?;
    let receipt = receive_receipt(&mut parent, &child, parent_started, overall)?;
    settle_parent(&mut parent, overall)?;
    let observation = read_capture(
        &artifacts,
        &qualification,
        &request,
        &receipt,
        child,
        overall,
    )?;
    parent.mark_settled();
    Ok(observation)
}

fn send_parent_plan(
    parent: &mut ParentGuard<'_>,
    artifacts: &OwnerArtifacts,
    request: &PreparedRequest,
    overall: Instant,
) -> Result<(), String> {
    let plan = ParentPlan {
        root: artifacts.store_root.to_string_lossy().into_owned(),
        request: request.clone(),
        initialize: false,
        launch: true,
        owner: id('5'),
        nonce: id('6'),
        gate: None,
        ledger: artifacts.ledger,
        fault: FixtureFault::None,
    };
    let frame = fixture::encode(&Frame::Parent(plan)).map_err(show)?;
    parent
        .child()
        .write_fixture_frame(&frame, bounded(overall, 5))
        .map_err(show)?;
    parent.child().close_stdin();
    parent.wait_for_post(bounded(overall, 90))
}

fn observe_held_post(
    artifacts: &OwnerArtifacts,
    qualification: &ClaudeQualification,
    request: &PreparedRequest,
    overall: Instant,
) -> Result<hf_domain::process::ProcessIdentity, String> {
    let store = Store::open(
        &artifacts.store_root,
        Limits::default(),
        bounded(overall, 5),
        hf_platform::lease::shared(),
    )
    .map_err(show)?;
    let snapshot = store
        .ownership_observe(&request.operation, bounded(overall, 5))
        .map_err(show)?
        .ok_or("owner has no durable request at validated POST")?;
    if snapshot.request != *request
        || !matches!(
            snapshot.phase,
            hf_domain::ownership::Phase::SpawnObserved
                | hf_domain::ownership::Phase::AcceptanceRecorded
        )
    {
        return Err("durable owner request/source differs from frozen HFCQ1".into());
    }
    let child = snapshot
        .child
        .ok_or("owner did not persist child before POST")?;
    drop(store);
    let hold = RootHold::open(
        &artifacts.store_root,
        bounded(overall, 5),
        hf_platform::lease::shared(),
    )
    .map_err(show)?;
    if hold.source_stamp() != &qualification.source {
        return Err("source stamp changed before held-POST identity observation".into());
    }
    drop(hold);
    let path = hf_platform::process::observed_executable_path(&child, bounded(overall, 5))
        .map_err(show)?;
    if path != Path::new(VENDOR_PATH) {
        return Err(format!(
            "kernel executable path mismatch: {}",
            path.display()
        ));
    }
    eprintln!(
        "phase=owner-post-hold child={} kernel-executable-path={} observation=on-disk-kernel-path-not-mapped-image-proof",
        identity_summary(&child),
        path.display()
    );
    Ok(child)
}

fn receive_receipt(
    parent: &mut ParentGuard<'_>,
    child: &hf_domain::process::ProcessIdentity,
    parent_started: Instant,
    overall: Instant,
) -> Result<hf_domain::prepared_launch::CaptureAccepted, String> {
    let until = overall.min(parent_started + Duration::from_secs(130));
    let frame =
        fixture::decode(&parent.child().read_fixture_frame(until).map_err(show)?).map_err(show)?;
    let Frame::CaptureAccepted(receipt) = frame else {
        return Err(format!(
            "owner qualification ended without CaptureAccepted: {frame:?}"
        ));
    };
    let receipt = *receipt;
    if receipt.capture.binding.child != *child {
        return Err("accepted capture child differs from post-held vendor process".into());
    }
    eprintln!(
        "phase=qualification-receipt operation={:?} attempt={:?} owner={:?} owner-process={} child={} source={:?} fence={:?} capture-revision={} stdout-extent={} stderr-extent={} stdout-eof={} stderr-eof={} loss={} exit={:?}",
        receipt.invitation.operation,
        receipt.ownership.request.attempt,
        receipt.invitation.owner,
        identity_summary(&receipt.invitation.process),
        identity_summary(&receipt.capture.binding.child),
        receipt.source,
        receipt.ownership.fence(),
        receipt.capture.progress.revision,
        receipt.capture.progress.stdout,
        receipt.capture.progress.stderr,
        receipt.capture.progress.stdout_eof,
        receipt.capture.progress.stderr_eof,
        receipt.capture.progress.loss,
        receipt.capture.exit
    );
    Ok(receipt)
}

fn settle_parent(parent: &mut ParentGuard<'_>, overall: Instant) -> Result<(), String> {
    let waited = parent
        .child()
        .wait_until(bounded(overall, 130), || false)
        .map_err(show)?;
    if waited != WaitOutcome::Exited(ProcessExit::Code(0)) {
        return Err(format!("parent did not settle successfully: {waited:?}"));
    }
    while !parent.child().capture().stdout.eof || !parent.child().capture().stderr.eof {
        parent
            .child()
            .pump(bounded(overall, 5), 4096)
            .map_err(show)?;
    }
    let (_, stderr) = parent.child().take_capture_bytes();
    if !stderr.is_empty()
        || parent.child().capture().stdout.overflow
        || parent.child().capture().stderr.overflow
    {
        return Err("parent transport stderr or capture loss".into());
    }
    parent
        .child()
        .fixture_eof(bounded(overall, 3))
        .map_err(show)
}

fn read_capture(
    artifacts: &OwnerArtifacts,
    qualification: &ClaudeQualification,
    request: &PreparedRequest,
    receipt: &hf_domain::prepared_launch::CaptureAccepted,
    child: hf_domain::process::ProcessIdentity,
    overall: Instant,
) -> Result<Observation, String> {
    let scratch = artifacts.root.join("owner-read-scratch");
    fs::create_dir(&scratch).map_err(show)?;
    let reader = ExactCaptureReader::new(
        RecoveryReadSession::new(
            &artifacts.store_root,
            &scratch,
            Limits::default(),
            hf_platform::lease::shared(),
        ),
        hf_platform::recovery::ExactRawCaptureReader::new(&artifacts.store_root),
        Sha256Digest,
    );
    let context = CaptureContext {
        invitation: &receipt.invitation,
        request,
        source: &qualification.source,
    };
    let raw = reader
        .read(
            context,
            &Delivery(Some(receipt.clone())),
            bounded(overall, 5),
        )
        .map_err(show)?;
    let gate = hf_platform::prepared_launch::observe_binary(&binary(), &Sha256Digest, overall)
        .map_err(show)?;
    if gate != qualification.gate_binary {
        return Err("gate binary pin changed after owner-hosted protocol".into());
    }
    eprintln!(
        "phase=qualification-capture raw-stdout-sha256={:?} raw-stderr-sha256={:?} raw-framed-sha256={:?}",
        raw.receipt.stdout_digest, raw.receipt.stderr_digest, raw.framed_digest
    );
    Ok(Observation {
        stdout: raw.bytes.stdout,
        stderr: raw.bytes.stderr,
        child,
    })
}
