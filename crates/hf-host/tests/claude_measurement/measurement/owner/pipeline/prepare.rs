#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;

pub(super) struct OwnerArtifacts {
    pub(super) root: PathBuf,
    pub(super) store_root: PathBuf,
    pub(super) ledger: QualifiedLedger,
    pub(super) source: hf_domain::recovery::SourceStamp,
    pub(super) topology: hf_domain::claude_qualification::RecordedFixtureTopology,
    pub(super) profile: String,
    pub(super) port: u16,
    pub(super) gate_binary: hf_domain::prepared_launch::BinaryPin,
    pub(super) vendor_binary: hf_domain::prepared_launch::BinaryPin,
}

struct StoreAssets {
    root: PathBuf,
    store_root: PathBuf,
    ledger: QualifiedLedger,
    source: hf_domain::recovery::SourceStamp,
    topology: hf_domain::claude_qualification::RecordedFixtureTopology,
}

pub(super) fn prepare_artifacts(
    measurement: &Measurement,
    overall: Instant,
    initial_vendor: &hf_domain::prepared_launch::BinaryPin,
) -> Result<OwnerArtifacts, String> {
    let store = prepare_store(measurement, overall)?;
    let profile =
        recorded_vendor_profile(&store.topology, measurement.server.port()).map_err(show)?;
    if profile.as_bytes() != measurement.vendor_profile.as_bytes() {
        return Err("preflight and qualification profile bytes differ".into());
    }
    let (gate_binary, vendor_binary) = observe_pins(overall, initial_vendor)?;
    Ok(OwnerArtifacts {
        root: store.root,
        store_root: store.store_root,
        ledger: store.ledger,
        source: store.source,
        topology: store.topology,
        profile,
        port: measurement.server.port(),
        gate_binary,
        vendor_binary,
    })
}

fn prepare_store(measurement: &Measurement, overall: Instant) -> Result<StoreAssets, String> {
    let root = measurement.owned.fixture.root().to_path_buf();
    let store_root = root.join("store");
    let store = Store::initialize_fixture(
        &store_root,
        Limits::default(),
        bounded(overall, 5),
        hf_platform::lease::shared(),
    )
    .map_err(show)?;
    drop(store);
    let mut ledger_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("r03-b2.effects"))
        .map_err(show)?;
    ledger_file
        .write_all(b"fixture owner qualification ledger\n")
        .map_err(show)?;
    ledger_file.sync_all().map_err(show)?;
    let metadata = ledger_file.metadata().map_err(show)?;
    let ledger = QualifiedLedger {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    drop(ledger_file);
    let hold = RootHold::open(
        &store_root,
        bounded(overall, 5),
        hf_platform::lease::shared(),
    )
    .map_err(show)?;
    let source = hold.source_stamp().clone();
    drop(hold);
    let topology = measurement
        .owned
        .fixture
        .recorded_topology()
        .map_err(show)?;
    Ok(StoreAssets {
        root,
        store_root,
        ledger,
        source,
        topology,
    })
}

fn observe_pins(
    overall: Instant,
    initial_vendor: &hf_domain::prepared_launch::BinaryPin,
) -> Result<
    (
        hf_domain::prepared_launch::BinaryPin,
        hf_domain::prepared_launch::BinaryPin,
    ),
    String,
> {
    let until = bounded(overall, 15);
    let gate = hf_platform::prepared_launch::observe_binary(&binary(), &Sha256Digest, until)
        .map_err(show)?;
    let vendor = hf_platform::prepared_launch::observe_qualification_vendor(
        Path::new(VENDOR_PATH),
        &Sha256Digest,
        until,
    )
    .map_err(show)?;
    if &vendor != initial_vendor {
        return Err("vendor pin changed between version bookend and owner preparation".into());
    }
    Ok((gate, vendor))
}

pub(super) fn spawn_parent<'a>(
    measurement: &'a mut Measurement,
    artifacts: &OwnerArtifacts,
    overall: Instant,
) -> Result<ParentGuard<'a>, String> {
    let child = RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: binary(),
            cwd: artifacts.root.clone(),
            args: vec![hf_domain::claude_qualification::PARENT_MODE.into()],
            environment: Vec::<(OsString, OsString)>::new(),
            capture_limit: 24576,
        },
        &MacIdentity,
        bounded(overall, 5),
    )
    .map_err(show)?;
    Ok(ParentGuard {
        child,
        server: &mut measurement.server,
        until: overall,
        settled: false,
    })
}

pub(super) fn parent_identity(
    parent: &mut ParentGuard<'_>,
) -> Result<hf_domain::process::ProcessIdentity, String> {
    if parent.child().initialization() != SpawnInitialization::Ready {
        return Err("qualification parent spawn identity was not registered".into());
    }
    let identity = parent
        .child()
        .registered_identity()
        .cloned()
        .ok_or("parent identity absent")?;
    eprintln!(
        "phase=qualification-parent identity={}",
        identity_summary(&identity)
    );
    Ok(identity)
}

pub(super) fn build_qualification(
    assets: &OwnerArtifacts,
    parent_identity: hf_domain::process::ProcessIdentity,
    nonce: String,
) -> Result<ClaudeQualification, String> {
    let policy = PreparationEnvelope {
        profile_digest: Digest::new(hf_interfaces::owner_preparation::PROFILE_DIGEST.into())
            .map_err(show)?,
        configuration_digest: digest('a'),
        operation: id('1'),
        session: id('2'),
        reservation: id('3'),
        attempt: id('4'),
        generation: assets.source.generation.clone(),
        policy: RuntimePolicy {
            background_requirement: Requirement::Required,
            permission_mode: PermissionMode::OsEnforced,
            background_idle_seconds: 600.0,
            tool_level_opt_in: false,
        },
        authority: AuthorityContext {
            caller: parent_identity,
            role: AuthorityRole::Direct,
            checkpoint: None,
            revision: None,
            issued_attempt: id('4'),
            grant_digest: digest('b'),
        },
        assignment: FixtureAssignment::FixedEffect,
    };
    let mut qualification = ClaudeQualification {
        policy,
        gate_binary: assets.gate_binary.clone(),
        vendor_binary: assets.vendor_binary.clone(),
        vendor_version: VENDOR_VERSION.into(),
        logical_hint: "/Users/nayanda/.local/bin/claude".into(),
        cwd: assets.root.to_string_lossy().into_owned(),
        source: assets.source.clone(),
        ledger: assets.ledger,
        topology: assets.topology.clone(),
        nonce,
        port: assets.port,
        profile: assets.profile.as_bytes().to_vec(),
        profile_digest: Sha256Digest.digest_parts(&[assets.profile.as_bytes()]),
        command: QualificationCommand {
            executable: String::new(),
            args: Vec::new(),
            environment: Vec::new(),
        },
    };
    qualification.command =
        hf_backends::claude_qualification::derive_command(&qualification).map_err(show)?;
    qualification.policy.configuration_digest =
        claude_qualification::configuration_digest(&qualification, &Sha256Digest).map_err(show)?;
    Ok(qualification)
}

pub(super) fn build_request(
    qualification: &ClaudeQualification,
) -> Result<PreparedRequest, String> {
    let payload = claude_qualification::encode(qualification, &Sha256Digest).map_err(show)?;
    let request = PreparedRequest {
        operation: qualification.policy.operation.clone(),
        session: qualification.policy.session.clone(),
        reservation: qualification.policy.reservation.clone(),
        attempt: qualification.policy.attempt.clone(),
        generation: qualification.policy.generation.clone(),
        digest: Sha256Digest.digest_parts(&[&payload]),
        payload,
    };
    eprintln!(
        "phase=qualification-prepared operation={:?} attempt={:?} generation={:?} source={:?} config-sha256={:?} profile-sha256={:?} request-sha256={:?} gate-sha256={:?} vendor-sha256={:?} vendor-length={} nonce={} port={}",
        request.operation,
        request.attempt,
        request.generation,
        qualification.source,
        qualification.policy.configuration_digest,
        qualification.profile_digest,
        request.digest,
        qualification.gate_binary.digest,
        qualification.vendor_binary.digest,
        qualification.vendor_binary.metadata.length,
        qualification.nonce,
        qualification.port
    );
    Ok(request)
}
