//! Public admission and a single private T0; no backend is executed here.
use super::HeldRoot;
use hf_application::{
    ctl::task_run::{RunFailure, RunRequest},
    owner_runtime::DigestPort,
};
use hf_domain::{
    ctl::state_root::StateRoot, ownership::PreparedRequest, task_runtime::LaunchIntentCreate,
};
use hf_storage::{Limits, RootHold, Store, WriterOptions};
use std::{
    path::Path,
    time::{Duration, Instant},
};
fn refused() -> RunFailure {
    RunFailure::Refused("Standalone launch admission refused or incomplete.".into())
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
    super::dispatch_request::preflight(path, &admitted.repo).map_err(|_| refused())?;
    let options = WriterOptions::suppressed().with_flush(hf_platform::durability::port(
        hf_platform::durability::DEFAULT_DURABILITY,
    ));
    let store = match std::fs::symlink_metadata(path) {
        Ok(_) => Store::open_with(
            path,
            Limits::default(),
            deadline,
            hf_platform::lease::shared(),
            options,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::initialize_runtime(
            path,
            Limits::default(),
            deadline,
            hf_platform::lease::shared(),
            options,
        ),
        Err(_) => return Err(refused()),
    }
    .map_err(|_| refused())?;
    super::dispatch_request::inventory(path, &store, &admitted.caller, deadline)
        .map_err(|_| refused())?;
    let original =
        RootHold::open(path, deadline, hf_platform::lease::shared()).map_err(|_| refused())?;
    let invitation =
        super::dispatch_request::invitation(&admitted, original.source_stamp().clone())
            .map_err(|_| refused())?;
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
        metadata: admitted.metadata,
        metadata_digest: invitation.metadata_digest.clone(),
        prompt: request.prompt.clone(),
        prompt_digest: invitation.prompt_digest.clone(),
    };
    let hold = HeldRoot::open(path, &invitation, deadline).map_err(|_| refused())?;
    if hf_platform::task_runtime::caller::current_caller(deadline)
        .ok()
        .as_ref()
        != Some(&admitted.caller)
    {
        return Err(refused());
    }
    original.revalidate().map_err(|_| refused())?;
    drop(store);
    // Once attempted, a missing transaction acknowledgement is conservatively unknown.
    super::prepare(&hold, &preparation, &intent, deadline).map_err(|_| unknown())?;
    super::dispatch_child::launch(
        path,
        hold,
        invitation,
        intent,
        admitted.environment,
        deadline,
    )
    .map_err(|error| match error {
        super::Error::NotStarted(reason) => {
            RunFailure::Refused(format!("Task did not start: {reason}."))
        }
        _ => unknown(),
    })
}
