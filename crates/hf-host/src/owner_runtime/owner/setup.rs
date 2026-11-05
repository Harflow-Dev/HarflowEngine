use super::*;
pub(super) fn handshake(channel: &PrivateIo) -> Result<Prepared, String> {
    let actual = MacIdentity
        .identity(std::process::id())
        .ok_or("owner identity unknown")?;
    channel
        .write_packet(&wire::encode(&Frame::Hello(actual.clone())).map_err(error)?)
        .map_err(error)?;
    let Frame::Invitation(invitation) =
        wire::decode(&channel.read_packet().map_err(error)?).map_err(error)?
    else {
        return Err("owner invitation required".into());
    };
    if invitation.process != actual || invitation.revision != 0 {
        return Err("owner invitation identity".into());
    }
    channel
        .write_packet(&wire::encode(&Frame::Ready(invitation.clone())).map_err(error)?)
        .map_err(error)?;
    let Frame::Prepared(prepared) =
        wire::decode(&channel.read_packet().map_err(error)?).map_err(error)?
    else {
        return Err("owner preparation required".into());
    };
    channel.expect_eof().map_err(error)?;
    if prepared.invitation != invitation {
        return Err("owner invitation changed".into());
    }
    validate(&prepared)?;
    Ok(prepared)
}
fn validate(p: &Prepared) -> Result<(), String> {
    let e = prep::decode(&p.request, &Sha256Digest).map_err(error)?;
    if current_parent_identity().as_ref() != Some(&e.authority.caller)
        || MacIdentity.identity(std::process::id()).as_ref() != Some(&p.invitation.process)
        || p.invitation.operation != p.request.operation
        || p.invitation.generation != p.request.generation
        || p.invitation.digest != p.request.digest
        || p.invitation.revision != 0
        || !matches!(p.fault, FixtureFault::None | FixtureFault::OutputSaturation)
    {
        return Err("owner preparation binding refused".into());
    }
    let root = Path::new(&p.root);
    let parent = root.parent().ok_or("owner root parent")?;
    if root.file_name().and_then(|s| s.to_str()) != Some("store")
        || std::fs::canonicalize(parent).map_err(error)? != parent
        || std::env::current_dir().map_err(error)? != parent
        || p.gate
            .as_ref()
            .is_some_and(|g| Path::new(&g.path).parent() != Some(parent))
        || qualify_ledger(root).map_err(error)? != p.ledger
    {
        return Err("owner physical root/ledger".into());
    }
    Ok(())
}
pub(super) fn start(prepared: Prepared, until: Instant) -> Result<OwnerLoop, String> {
    let executable = std::env::current_exe().map_err(error)?;
    start_at(prepared, until, &executable)
}
pub(super) fn start_at(
    prepared: Prepared,
    until: Instant,
    executable: &Path,
) -> Result<OwnerLoop, String> {
    validate(&prepared)?;
    let root = Path::new(&prepared.root);
    let cwd = root.parent().ok_or("owner root parent")?.to_path_buf();
    let hold = RootHold::open(root, until, hf_platform::lease::shared()).map_err(error)?;
    if hold.source_stamp().generation != prepared.request.generation {
        return Err("owner root generation".into());
    }
    let executable = executable.to_path_buf();
    let mut resources = Resources::new(hold);
    let result = actor(&prepared, &cwd, &executable, &mut resources, until);
    let actor = match result {
        Ok(a) => a,
        Err(e) => {
            resources.retain(None);
            return Err(e);
        }
    };
    Ok(OwnerLoop {
        resources,
        actor,
        prepared,
        cwd,
        executable,
        capture: Capture::default(),
        input: None,
        input_done: false,
        accepted: false,
        capture_started: false,
        closed: false,
        until,
        progress: Progress::default(),
    })
}
fn actor(
    p: &Prepared,
    cwd: &Path,
    executable: &Path,
    resources: &mut Resources,
    until: Instant,
) -> Result<Actor, String> {
    resources.writer = Some(spawn_at(
        "--internal-storage-writer-fixture-v1",
        cwd,
        executable,
        until,
    )?);
    let writer = resources.writer.as_mut().ok_or("writer absent")?;
    let identity = ready(writer)?;
    let owner = OwnerClaim {
        owner: p.invitation.owner.clone(),
        nonce: p.invitation.nonce.clone(),
        process: p.invitation.process.clone(),
    };
    let envelope = prep::decode(&p.request, &Sha256Digest).map_err(error)?;
    let participant = StorageParticipant {
        source: resources.hold.source_stamp().clone(),
        operation: p.request.operation.clone(),
        attempt: p.request.attempt.clone(),
        request_digest: p.request.digest.clone(),
        configuration_digest: envelope.configuration_digest,
        profile_digest: envelope.profile_digest,
        owner: owner.clone(),
        writer: identity.clone(),
        channel_nonce: p.invitation.nonce.clone(),
    };
    let binding = participant_digest(&participant, &Sha256Digest);
    let io = writer.take_private_storage_io(&identity).map_err(error)?;
    let mut actor = Actor::new(
        io,
        StorageCoordinator::new(p.request.clone(), owner, binding),
        until,
    );
    let bootstrap = StorageCommand::Bootstrap(Box::new(StorageBootstrap {
        root: p.root.clone(),
        request: p.request.clone(),
        participant,
        gate: p.gate.clone(),
    }));
    if let Err(e) = actor.submit(bootstrap) {
        resources.retain(Some(&mut actor));
        return Err(e);
    }
    Ok(actor)
}
pub(super) fn spawn_at(
    mode: &str,
    cwd: &Path,
    executable: &Path,
    until: Instant,
) -> Result<RetainedOwnedChild, String> {
    RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: executable.to_path_buf(),
            cwd: cwd.to_path_buf(),
            args: vec![mode.into()],
            environment: Vec::new(),
            capture_limit: 24576,
        },
        &MacIdentity,
        until,
    )
    .map_err(error)
}
pub(super) fn ready(child: &RetainedOwnedChild) -> Result<ProcessIdentity, String> {
    if child.initialization() != SpawnInitialization::Ready {
        return Err("child initialization unknown".into());
    }
    child
        .registered_identity()
        .cloned()
        .ok_or("child identity absent".into())
}
