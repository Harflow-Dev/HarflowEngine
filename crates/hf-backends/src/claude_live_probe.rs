//! Pure direct-child live diagnostic construction; no production admission.
use crate::claude_measurement::{
    ClaudeFixtureCommand, ClaudeFixtureRequest, FixtureRequestError, claude_fixture_command,
};

/// The platform must clear the environment, enforce isolation, and append only
/// the separately validated volatile OAuth access token. Configured turn, retry,
/// output and budget controls do not prove an HTTP request count or billing cap.
pub fn live_command(
    request: &ClaudeFixtureRequest,
    proxy_port: u16,
) -> Result<ClaudeFixtureCommand, FixtureRequestError> {
    if proxy_port == 0 || proxy_port != request.port {
        return Err(FixtureRequestError::InvalidEndpoint);
    }
    let mut command = claude_fixture_command(request)?;
    command.args.retain(|arg| arg != "--bare");
    command
        .args
        .splice(0..0, ["--max-turns".to_owned(), "1".to_owned()]);
    command
        .env
        .retain(|(key, _)| key != "ANTHROPIC_API_KEY" && key != "ANTHROPIC_BASE_URL");
    let proxy = format!("http://127.0.0.1:{proxy_port}");
    command.env.push(("HTTPS_PROXY".into(), proxy.clone()));
    command.env.push(("HTTP_PROXY".into(), proxy));
    command.env.extend(live_environment());
    command.source_profile = "claude-live-direct-child-auth-egress-v1; managed settings may apply; unsupported startup must refuse";
    Ok(command)
}

fn live_environment() -> impl Iterator<Item = (String, String)> {
    [
        ("ICU_TIMEZONE_FILES_DIR", ""),
        ("CLAUDE_CODE_MAX_RETRIES", "0"),
        ("MAX_STRUCTURED_OUTPUT_RETRIES", "0"),
        ("CLAUDE_CODE_NONSTREAMING_TIMEOUT_RETRIES", "0"),
        ("CLAUDE_CODE_MAX_OUTPUT_TOKENS", "128"),
        ("MAX_THINKING_TOKENS", "0"),
        ("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS", "1"),
        ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
}
