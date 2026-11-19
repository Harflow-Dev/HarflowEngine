//! Public admission and a single private T0; no backend is executed here.
use super::{HeldRoot, dispatch_request::Admitted};
use hf_application::{
    ctl::task_run::{RunFailure, RunRequest},
    owner_runtime::DigestPort,
};
use hf_domain::{
    ctl::state_root::StateRoot,
    ownership::PreparedRequest,
    task_runtime::{LaunchIntentCreate, ProductionInvitation},
};
use hf_platform::task_runtime::raw::{PrivateRootError, open_private_root, private_root_error};
use hf_storage::{Limits, RootHold, Store};
use std::{
    path::Path,
    time::{Duration, Instant},
};
fn refused() -> RunFailure {
    RunFailure::Refused("Standalone launch admission refused or incomplete.".into())
}
/// The first check failed: the root exists but is not a private directory of the user.
fn root_mode_refused(path: &Path, error: &std::io::Error) -> RunFailure {
    if private_root_error(error) == PrivateRootError::LooseMode {
        return RunFailure::Refused(format!(
            "State root {} allows group or other access; it must be a directory owned by the \
             current user with no group or other permissions. No state was changed.",
            path.display()
        ));
    }
    RunFailure::Refused(format!(
        "State root {} must be an existing, non-symlinked directory at its canonical path, \
         owned by the current user, with no group or other permissions. No state was changed.",
        path.display()
    ))
}
/// The root failed the same predicate after initialization or changed identity.
fn root_changed_refused(path: &Path, error: Option<&std::io::Error>) -> RunFailure {
    if error.is_some_and(|e| private_root_error(e) == PrivateRootError::LooseMode) {
        return RunFailure::Refused(format!(
            "State root {} allows group or other access; it must be owned by the current user \
             with no group or other permissions. No launch intent was recorded.",
            path.display()
        ));
    }
    RunFailure::Refused(format!(
        "State root {} must be an existing, non-symlinked directory at its canonical path, \
         owned by the current user, with no group or other permissions, and unchanged since \
         it was opened. No launch intent was recorded.",
        path.display()
    ))
}
fn unknown() -> RunFailure {
    RunFailure::Unknown(
        "Launch outcome is unknown; execution may exist. Do not retry automatically.".into(),
    )
}
/// Process-bound entry: the caller must exit ctl immediately after rendering its reply.
/// Retained original owner wait custody is intentionally kept until that process exit.
pub fn run_for_ctl_exit(root: &StateRoot, request: &RunRequest) -> Result<String, RunFailure> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let admitted = super::dispatch_request::admit(request, deadline).map_err(|_| refused())?;
    let path = Path::new(root.as_str());
    super::dispatch_policy::preflight(path, &admitted.repo).map_err(|_| refused())?;
    let store = open_checked(path, &admitted, deadline)?;
    commit_and_launch(path, store, admitted, request, deadline)
}
/// Opens the root and applies the caller-policy inventory. An existing root is inventoried
/// before anything changes. A missing root has no recorded participants, so the caller is
/// observed once more and only then is the root created; a refusal creates no root.
fn open_checked(path: &Path, admitted: &Admitted, deadline: Instant) -> Result<Store, RunFailure> {
    let options = super::store_options::suppressed();
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            open_private_root(path).map_err(|error| root_mode_refused(path, &error))?;
            let store = Store::open_with(
                path,
                Limits::default(),
                deadline,
                hf_platform::lease::shared(),
                options,
            )
            .map_err(|_| refused())?;
            super::dispatch_policy::inventory(path, &store, &admitted.caller, deadline)
                .map_err(|_| refused())?;
            Ok(store)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            super::dispatch_policy::reobserve(&admitted.caller, deadline).map_err(|_| refused())?;
            Store::initialize_runtime(
                path,
                Limits::default(),
                deadline,
                hf_platform::lease::shared(),
                options,
            )
            .map_err(|_| refused())
        }
        Err(_) => Err(refused()),
    }
}
/// Commits the prepared launch intent and hands custody to the original owner.
fn commit_and_launch(
    path: &Path,
    store: Store,
    admitted: Admitted,
    request: &RunRequest,
    deadline: Instant,
) -> Result<String, RunFailure> {
    let original =
        RootHold::open(path, deadline, hf_platform::lease::shared()).map_err(|_| refused())?;
    let Launch {
        invitation,
        preparation,
        intent,
    } = build_launch(&admitted, request, original.source_stamp().clone())?;
    let hold = HeldRoot::open(path, &invitation, deadline).map_err(|_| refused())?;
    if super::caller_evidence::current(deadline).ok().as_ref() != Some(&admitted.caller) {
        return Err(refused());
    }
    original.revalidate().map_err(|_| refused())?;
    #[cfg(test)]
    super::seams::dispatch_hit(
        super::seams::DispatchPoint::BeforeIntentRootCheck,
        &invitation.binding,
    )
    .map_err(|_| refused())?;
    // The root may have been created by initialization above; it must still be the private
    // directory the stamp names before any launch intent is recorded.
    match open_private_root(path) {
        Ok(root) if root.identity() == &original.source_stamp().root => {}
        Ok(_) => return Err(root_changed_refused(path, None)),
        Err(error) => return Err(root_changed_refused(path, Some(&error))),
    }
    drop(store);
    // Once attempted, a missing transaction acknowledgement is conservatively unknown.
    super::prepare(&hold, &preparation, &intent, deadline).map_err(|_| unknown())?;
    #[cfg(test)]
    super::seams::dispatch_hit(super::seams::DispatchPoint::AfterT0, &invitation.binding)
        .map_err(|_| unknown())?;
    super::dispatch_child::launch(
        path,
        hold,
        invitation,
        intent,
        admitted.environment,
        deadline,
    )
    .map_err(classify)
}
struct Launch {
    invitation: ProductionInvitation,
    preparation: PreparedRequest,
    intent: LaunchIntentCreate,
}
/// The invitation, its prepared T0 request and the launch intent of one admitted run.
fn build_launch(
    admitted: &Admitted,
    request: &RunRequest,
    source: hf_domain::recovery::SourceStamp,
) -> Result<Launch, RunFailure> {
    let invitation =
        super::dispatch_request::invitation(admitted, source).map_err(|_| refused())?;
    let payload =
        hf_interfaces::task_runtime::encode_invitation(&invitation).map_err(|_| refused())?;
    let preparation = PreparedRequest {
        operation: invitation.binding.operation.clone(),
        session: invitation.ownership_session.clone(),
        reservation: invitation.reservation.clone(),
        attempt: invitation.binding.attempt.clone(),
        generation: invitation.binding.generation.clone(),
        digest: hf_storage::Sha256Digest.digest_parts(&[&payload]),
        payload,
    };
    let intent = LaunchIntentCreate {
        task: invitation.binding.task.clone(),
        metadata: admitted.metadata.clone(),
        metadata_digest: invitation.metadata_digest.clone(),
        prompt: request.prompt.clone(),
        prompt_digest: invitation.prompt_digest.clone(),
    };
    Ok(Launch {
        invitation,
        preparation,
        intent,
    })
}
pub(super) fn classify(error: super::Error) -> RunFailure {
    match error {
        super::Error::NotStarted(reason) => {
            RunFailure::Refused(format!("Task did not start: {reason}."))
        }
        _ => unknown(),
    }
}
