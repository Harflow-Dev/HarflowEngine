//! Immutable pre-effect request, process and binary observations.
use super::Error;
use hf_application::{
    ctl::{state_inspect::OperationIdPort, task_run::RunRequest},
    owner_runtime::DigestPort,
};
use hf_domain::{
    ownership::{Digest, Id},
    prepared_launch::BinaryPin,
    recovery::SourceStamp,
    task_launch::CliSessionId,
    task_lifecycle::LifecycleBinding,
    task_read::TaskId,
    task_runtime::ProductionInvitation,
};
use hf_platform::task_runtime::caller::CallerObservation;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Instant,
};
pub(super) struct Admitted {
    pub repo: PathBuf,
    pub caller: CallerObservation,
    pub environment: Vec<(String, String)>,
    pub metadata: Vec<u8>,
    task: TaskId,
    session: CliSessionId,
    operation: Id,
    reservation: Id,
    attempt: Id,
    ownership_session: Id,
    backend: BinaryPin,
    ctl: BinaryPin,
    prompt_digest: Digest,
    environment_digest: Digest,
}
fn fresh() -> Result<String, Error> {
    hf_platform::ctl::operation_id::OsOperationIds
        .fresh()
        .ok_or(Error::Refused)
}
fn id() -> Result<Id, Error> {
    Id::new(fresh()?).map_err(|_| Error::Refused)
}
pub(super) fn admit(request: &RunRequest, deadline: Instant) -> Result<Admitted, Error> {
    let repo = validate_request(request)?;
    let caller = hf_platform::task_runtime::caller::current_caller(deadline)?;
    if caller.chain.len() < 2 {
        return Err(Error::Refused);
    }
    let (environment, encoded) = environment()?;
    let env_os: Vec<_> = environment
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
    let backend = hf_platform::backend_launch::observe_selection(
        &env_os,
        &hf_storage::Sha256Digest,
        deadline,
    )
    .map_err(|_| Error::Refused)?;
    let executable = std::fs::canonicalize(std::env::current_exe()?)?;
    let ctl = hf_platform::backend_launch::observe_binary_pin(
        &executable,
        &hf_storage::Sha256Digest,
        deadline,
    )
    .map_err(|_| Error::Refused)?;
    let raw = fresh()?;
    let uuid = format!(
        "{}-{}-4{}-a{}-{}",
        &raw[..8],
        &raw[8..12],
        &raw[13..16],
        &raw[17..20],
        &raw[20..]
    );
    let session = CliSessionId::new(uuid).map_err(|_| Error::Refused)?;
    let task = TaskId::new(fresh()?).map_err(|_| Error::Refused)?;
    let metadata = metadata(request, &caller, &task, &session)?;
    Ok(Admitted {
        repo,
        caller,
        environment,
        metadata,
        task,
        session,
        operation: id()?,
        reservation: id()?,
        attempt: id()?,
        ownership_session: id()?,
        backend,
        ctl,
        prompt_digest: hf_storage::Sha256Digest.digest_parts(&[request.prompt.as_bytes()]),
        environment_digest: hf_storage::Sha256Digest.digest_parts(&[&encoded]),
    })
}
pub(super) fn preflight(root: &Path, repo: &Path) -> Result<(), Error> {
    let temporary = std::env::temp_dir();
    hf_platform::task_runtime::space::preflight(
        &[root, repo, &temporary],
        Instant::now() + std::time::Duration::from_secs(2),
    )?;
    Ok(())
}
pub(super) fn inventory(
    _root: &Path,
    store: &hf_storage::Store,
    caller: &CallerObservation,
    deadline: Instant,
) -> Result<(), Error> {
    let denied = store.runtime_mutation_candidates(
        &hf_interfaces::task_runtime::CanonicalLaunchValidator,
        64,
        deadline,
    )?;
    if caller
        .chain
        .iter()
        .any(|link| denied.contains(&link.identity))
    {
        return Err(Error::Refused);
    }
    let ordinary = store.ordinary_runtime_mutation_candidates(64, deadline)?;
    if caller
        .chain
        .iter()
        .any(|link| ordinary.contains(&link.identity))
    {
        return Err(Error::Refused);
    }
    if hf_platform::task_runtime::caller::current_caller(deadline)? != *caller {
        return Err(Error::Refused);
    }
    Ok(())
}
pub(super) fn invitation(a: &Admitted, source: SourceStamp) -> Result<ProductionInvitation, Error> {
    Ok(ProductionInvitation {
        binding: LifecycleBinding {
            task: a.task.clone(),
            operation: a.operation.clone(),
            attempt: a.attempt.clone(),
            generation: source.generation.clone(),
        },
        reservation: a.reservation.clone(),
        ownership_session: a.ownership_session.clone(),
        session: a.session.clone(),
        source,
        caller: a.caller.chain[1].identity.clone(),
        dispatcher: a.caller.chain[0].identity.clone(),
        ctl: a.ctl.clone(),
        backend: a.backend.clone(),
        cwd: a.repo.to_str().ok_or(Error::Refused)?.into(),
        environment_digest: a.environment_digest.clone(),
        prompt_digest: a.prompt_digest.clone(),
        metadata_digest: hf_storage::Sha256Digest.digest_parts(&[&a.metadata]),
        configuration_digest: super::configuration_digest(),
        invocation_digest: super::invocation_digest(&a.session),
        expected_version: hf_backends::backend_launch::CLAUDE_VERSION.into(),
    })
}

fn validate_request(request: &RunRequest) -> Result<PathBuf, Error> {
    for key in [
        "HARFLOW_TASK_ID",
        "HARFLOW_ROOT_TASK_ID",
        "HARFLOW_DEPTH",
        "PB_TASK_ID",
        "PB_ROOT_TASK_ID",
        "PB_DEPTH",
    ] {
        if std::env::var_os(key).is_some() {
            return Err(Error::Refused);
        }
    }
    if request.backend != "claude"
        || request.freedom != "unrestricted"
        || request.prompt.trim().is_empty()
        || request.prompt.len() > 1_048_576
        || request.prompt.contains('\0')
    {
        return Err(Error::Refused);
    }
    let repo = std::fs::canonicalize(&request.repo)?;
    if !repo.is_dir()
        || repo != Path::new(&request.repo)
        || !repo.is_absolute()
        || request.repo.len() > 4096
        || request.repo.chars().any(char::is_control)
    {
        return Err(Error::Refused);
    }
    Ok(repo)
}

type EncodedEnvironment = (Vec<(String, String)>, Vec<u8>);
fn environment() -> Result<EncodedEnvironment, Error> {
    let mut environment: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| {
            Ok((
                k.into_string().map_err(|_| Error::Refused)?,
                v.into_string().map_err(|_| Error::Refused)?,
            ))
        })
        .collect::<Result<_, Error>>()?;
    environment.sort();
    let encoded = hf_interfaces::task_runtime::encode_environment(&environment)
        .map_err(|_| Error::Refused)?;
    let args_bound = 4096usize;
    if environment
        .iter()
        .map(|(k, v)| k.len() + v.len())
        .sum::<usize>()
        + args_bound
        > 65536
    {
        return Err(Error::Refused);
    }
    Ok((environment, encoded))
}

fn metadata(
    request: &RunRequest,
    caller: &CallerObservation,
    task: &TaskId,
    session: &CliSessionId,
) -> Result<Vec<u8>, Error> {
    let parent = &caller.chain[1].identity;
    let metadata = hf_interfaces::task_runtime::encode_metadata(
        &hf_interfaces::task_runtime::InitialLaunchMetadata {
            task: task.clone(),
            session: session.clone(),
            repo: request.repo.clone(),
            title: None,
            model: None,
            created_at: super::timestamp()?,
            caller_reference: format!(
                "{}:{}:{}:{}",
                parent.pid(),
                parent.start_seconds(),
                parent.start_microseconds(),
                parent.boot_uuid()
            ),
            configuration_reference: super::configuration_digest().as_str().into(),
        },
    )
    .map_err(|_| Error::Refused)?;
    Ok(metadata)
}
