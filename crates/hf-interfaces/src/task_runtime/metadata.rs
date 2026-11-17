use hf_application::task_runtime::*;
use hf_domain::{
    ownership::PreparedRequest, task_launch::CliSessionId, task_read::TaskId, task_runtime::*,
};
use serde_json::{Value, json};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialLaunchMetadata {
    pub task: TaskId,
    pub session: CliSessionId,
    pub repo: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
    pub caller_reference: String,
    pub configuration_reference: String,
}
pub struct CanonicalLaunchValidator;
pub fn encode_metadata(m: &InitialLaunchMetadata) -> Result<Vec<u8>, InvalidIntent> {
    if hf_domain::timestamp::parse_micros(&m.created_at).is_none()
        || m.repo.len() > 4096
        || !m.repo.starts_with('/')
        || m.repo.contains('\0')
        || m.title.as_ref().is_some_and(|s| s.len() > 2048)
        || m.model.as_ref().is_some_and(|s| s.len() > 2048)
        || [
            &m.created_at,
            &m.caller_reference,
            &m.configuration_reference,
        ]
        .iter()
        .any(|s| s.is_empty() || s.len() > 2048)
    {
        return Err(InvalidIntent);
    }
    let bytes=serde_json::to_vec(&json!({"version":1,"task":m.task.as_str(),"session":m.session.as_str(),"backend":"claude","repo":m.repo,"title":m.title,"model":m.model,"created_at":m.created_at,"caller":m.caller_reference,"configuration":m.configuration_reference,"permission":"unrestricted","policy":"first-public-task-v1"})).map_err(|_|InvalidIntent)?;
    if bytes.len() > 16384 {
        return Err(InvalidIntent);
    }
    Ok(bytes)
}
pub fn decode_metadata(bytes: &[u8]) -> Result<InitialLaunchMetadata, InvalidIntent> {
    if bytes.len() > 16384 {
        return Err(InvalidIntent);
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| InvalidIntent)?;
    let text = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(InvalidIntent)
    };
    let optional = |key: &str| match v.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(InvalidIntent),
    };
    let m = InitialLaunchMetadata {
        task: TaskId::new(text("task")?).map_err(|_| InvalidIntent)?,
        session: CliSessionId::new(text("session")?).map_err(|_| InvalidIntent)?,
        repo: text("repo")?,
        title: optional("title")?,
        model: optional("model")?,
        created_at: text("created_at")?,
        caller_reference: text("caller")?,
        configuration_reference: text("configuration")?,
    };
    if encode_metadata(&m)? != bytes {
        return Err(InvalidIntent);
    }
    Ok(m)
}
/// Digest-bound preparation envelope. Storage recomputes its SHA-256 before T0.
pub fn intent_payload(request: &PreparedRequest, intent: &LaunchIntentCreate) -> Vec<u8> {
    let mut bytes = b"HFTI\x01".to_vec();
    for value in [
        intent.task.as_str(),
        request.operation.as_str(),
        request.session.as_str(),
        request.reservation.as_str(),
        request.attempt.as_str(),
        request.generation.as_str(),
        intent.metadata_digest.as_str(),
        intent.prompt_digest.as_str(),
        "first-public-task-v1",
    ] {
        bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    bytes
}
impl InitialIntentValidator for CanonicalLaunchValidator {
    fn validate(
        &self,
        request: &PreparedRequest,
        intent: &LaunchIntentCreate,
    ) -> Result<(), InvalidIntent> {
        let m = decode_metadata(&intent.metadata)?;
        let invitation = super::invitation::decode_invitation(&request.payload)?;
        if m.task != intent.task
            || invitation.binding.task != intent.task
            || invitation.binding.operation != request.operation
            || invitation.binding.attempt != request.attempt
            || invitation.binding.generation != request.generation
            || invitation.reservation != request.reservation
            || invitation.ownership_session != request.session
            || invitation.session != m.session
            || invitation.cwd != m.repo
            || invitation.prompt_digest != intent.prompt_digest
            || invitation.metadata_digest != intent.metadata_digest
            || m.configuration_reference != invitation.configuration_digest.as_str()
            || m.caller_reference
                != format!(
                    "{}:{}:{}:{}",
                    invitation.caller.pid(),
                    invitation.caller.start_seconds(),
                    invitation.caller.start_microseconds(),
                    invitation.caller.boot_uuid()
                )
        {
            return Err(InvalidIntent);
        }
        // Refuse an unpublishable canonical projection before any child can exist.
        let initial = hf_application::task_lifecycle::InitialProjectionFacts {
            task: &m.task,
            backend: "claude",
            repo_path: &m.repo,
            session_id: m.session.as_str(),
            status: "running",
            started_at: "9999-12-31T23:59:59.999999+00:00",
            title: m.title.as_deref(),
            model: m.model.as_deref(),
            continuation_of: None,
            chain_root: None,
        };
        super::projection::spawn_projection(&initial)?;
        super::terminal::terminal_projection(
            &hf_application::task_runtime::TerminalProjectionFacts {
                initial: hf_application::task_lifecycle::InitialProjectionFacts {
                    status: "cancelled",
                    ..initial
                },
                exit_code: i64::MIN,
                finished_at: "9999-12-31T23:59:59.999999+00:00",
                output_drained: true,
            },
        )?;
        Ok(())
    }
}
impl SpawnProjectionValidator for CanonicalLaunchValidator {
    fn validate(
        &self,
        intent: &LaunchIntent,
        facts: &SpawnProjectionFacts<'_>,
        header: &[u8],
        status: &[u8],
        prompt: &str,
    ) -> Result<(), InvalidIntent> {
        InitialIntentValidator::validate(self, &intent.preparation, &intent.create)?;
        let m = decode_metadata(&intent.create.metadata)?;
        let f = &facts.initial;
        if f.task != &m.task
            || f.session_id != m.session.as_str()
            || f.backend != "claude"
            || f.repo_path != m.repo
            || f.title != m.title.as_deref()
            || f.model != m.model.as_deref()
            || f.continuation_of.is_some()
            || f.chain_root.is_some()
            || f.status != "running"
            || prompt != intent.create.prompt
        {
            return Err(InvalidIntent);
        }
        let (expected_header, expected_status) = super::projection::spawn_projection(f)?;
        if header != expected_header || status != expected_status {
            return Err(InvalidIntent);
        }
        Ok(())
    }
}
impl RuntimeParticipantDecoder for CanonicalLaunchValidator {
    fn participants(
        &self,
        request: &PreparedRequest,
    ) -> Result<Vec<hf_domain::process::ProcessIdentity>, InvalidIntent> {
        let v = super::invitation::decode_invitation(&request.payload)?;
        if v.binding.operation != request.operation
            || v.binding.attempt != request.attempt
            || v.binding.generation != request.generation
            || v.reservation != request.reservation
            || v.ownership_session != request.session
        {
            return Err(InvalidIntent);
        }
        Ok(vec![v.dispatcher])
    }
}
