//! Pre-effect request validation, environment capture and caller inventory.
use super::Error;
use hf_application::ctl::task_run::RunRequest;
use hf_platform::task_runtime::caller::CallerObservation;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
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

pub(super) fn validate_request(request: &RunRequest) -> Result<PathBuf, Error> {
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
pub(super) fn environment() -> Result<EncodedEnvironment, Error> {
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
