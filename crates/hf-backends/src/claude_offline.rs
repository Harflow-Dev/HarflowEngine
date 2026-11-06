//! Pure pinned offline planning and passive reduction; never launches a process.
use crate::claude_measurement::{ClaudeFixtureRequest, claude_fixture_command};
use hf_application::{backend_runtime::BackendObservationPort, owner_runtime::DigestPort};
use hf_domain::{backend::*, owner_policy::ResultState};

pub fn plan(
    request: &ClaudeFixtureRequest,
    prompt: &str,
    cwd: &str,
    digest: &impl DigestPort,
) -> Result<InvocationPlan, BackendError> {
    if prompt.is_empty() || prompt.len() > 8192 || prompt.contains('\0') || !valid_path(cwd) {
        return Err(BackendError::InvalidBinding);
    }
    let mut command = claude_fixture_command(request).map_err(|_| BackendError::InvalidBinding)?;
    command.args.pop();
    command.args.push(prompt.into());
    let mut plan = InvocationPlan {
        executable: command.executable,
        args: command.args,
        env: command.env,
        cwd: cwd.into(),
        source_profile: command.source_profile.into(),
        configuration_digest: digest.digest_parts(&[]),
    };
    plan.configuration_digest =
        hf_application::backend_runtime::configuration_digest(&plan, digest);
    Ok(plan)
}
fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() > 1
        && path.len() <= 4096
        && !path.chars().any(char::is_control)
        && !path.split('/').any(|p| p == "." || p == "..")
}

pub fn reduce(trace: &BackendTrace, binding: &BackendBinding) -> BackendSummary {
    let mut summary = BackendSummary {
        binding: binding.clone(),
        result: ResultState::Unknown,
        sticky_failure: false,
        usable: trace.fault.is_none() && trace.ended,
        terminal: false,
        pending_tools: 0,
        text: String::new(),
        cli_session: None,
    };
    let mut normalized_bytes = 0usize;
    let mut tools: Vec<(Option<String>, String)> = Vec::new();
    let mut seen_tools: Vec<(Option<String>, String)> = Vec::new();
    if trace.records.len() > 128 {
        summary.usable = false;
    }
    for record in trace.records.iter().take(128) {
        normalized_bytes = normalized_bytes.saturating_add(record_size(record));
        if !valid_record(record) || normalized_bytes > 65536 {
            summary.usable = false;
            continue;
        }
        if record.kind == RecordKind::Init && summary.cli_session.is_some() {
            summary.usable = false;
            continue;
        }
        if record.attempt != binding.request.attempt
            || record.cli_session.is_empty()
            || record.cli_session.len() > 256
        {
            summary.usable = false;
            continue;
        }
        if summary
            .cli_session
            .as_ref()
            .is_some_and(|s| s != &record.cli_session)
            || summary.terminal
        {
            summary.usable = false;
            continue;
        }
        if summary.cli_session.is_none() {
            if record.kind != RecordKind::Init || record.parent.is_some() {
                summary.usable = false;
                continue;
            }
            summary.cli_session = Some(record.cli_session.clone());
        }
        apply(record, &mut summary, &mut tools, &mut seen_tools);
    }
    summary.pending_tools = tools.len();
    if summary.sticky_failure {
        summary.result = ResultState::Failed;
    } else if !summary.usable || !summary.terminal || !tools.is_empty() {
        summary.result = ResultState::Unknown;
    }
    summary
}
fn apply(
    record: &BackendRecord,
    summary: &mut BackendSummary,
    tools: &mut Vec<(Option<String>, String)>,
    seen_tools: &mut Vec<(Option<String>, String)>,
) {
    match &record.kind {
        RecordKind::Init | RecordKind::Metadata => (),
        RecordKind::Text(text) if record.parent.is_none() => {
            if summary.text.len().saturating_add(text.len()) > 16384 {
                summary.usable = false;
            } else {
                summary.text.push_str(text);
            }
        }
        RecordKind::ToolUse(id) => {
            let key = (record.parent.clone(), id.clone());
            if id.is_empty()
                || id.len() > 256
                || seen_tools.contains(&key)
                || seen_tools.len() >= 128
            {
                summary.usable = false;
            } else {
                seen_tools.push(key.clone());
                tools.push(key);
            }
        }
        RecordKind::ToolResult { id, .. } => {
            let key = (record.parent.clone(), id.clone());
            if let Some(index) = tools.iter().position(|k| k == &key) {
                tools.remove(index);
            } else {
                summary.usable = false;
            }
        }
        RecordKind::Terminal { failed } if record.parent.is_none() => {
            summary.terminal = true;
            summary.sticky_failure |= failed;
            summary.result = if *failed {
                ResultState::Failed
            } else {
                ResultState::Succeeded
            };
        }
        _ => (),
    }
}
/// Finite synthetic trace adapter; no native process or input port implementation.
pub struct RecordedAdapter {
    pub trace: Option<BackendTrace>,
}
impl BackendObservationPort for RecordedAdapter {
    fn observe(&mut self) -> Result<BackendTrace, BackendError> {
        self.trace.take().ok_or(BackendError::Source)
    }
}

fn bounded_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
fn valid_record(r: &BackendRecord) -> bool {
    bounded_id(&r.cli_session)
        && r.parent.as_deref().is_none_or(bounded_id)
        && match &r.kind {
            RecordKind::Text(s) => s.len() <= 8192,
            RecordKind::ToolUse(s) | RecordKind::ToolResult { id: s, .. } => bounded_id(s),
            _ => true,
        }
}
fn record_size(r: &BackendRecord) -> usize {
    r.cli_session
        .len()
        .saturating_add(r.parent.as_ref().map_or(0, String::len))
        .saturating_add(match &r.kind {
            RecordKind::Text(s) | RecordKind::ToolUse(s) | RecordKind::ToolResult { id: s, .. } => {
                s.len()
            }
            _ => 0,
        })
}
