//! Isolated no-tools/no-fork fixture construction, never a production backend route.
use hf_domain::backend_measurement::StrictRouteUnsupported;

const FIXTURE_INSTRUCTIONS: &str = "Reply with exactly the marker requested by the user. Do not use tools, delegate, or access files.";

/// Paths must be fresh private harness-owned physical directories. This factory
/// validates lexical shape only; the platform must prove topology and isolation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeFixtureRequest {
    pub logical_executable: String,
    pub home: String,
    pub config: String,
    pub xdg_config: String,
    pub xdg_cache: String,
    pub temporary: String,
    pub claude_temporary: String,
    pub port: u16,
    pub nonce: String,
}

/// Explicit vendor command only. The platform supplies the fixed sandbox/env -C
/// wrapper, exact profile and safe spawn; this value cannot itself launch anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeFixtureCommand {
    pub executable: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub source_profile: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureRequestError {
    InvalidPrivatePath,
    InvalidEndpoint,
}

pub fn claude_fixture_command(
    request: &ClaudeFixtureRequest,
) -> Result<ClaudeFixtureCommand, FixtureRequestError> {
    validate_request(request)?;
    Ok(ClaudeFixtureCommand {
        executable: request.logical_executable.clone(),
        args: fixture_args(&request.nonce),
        env: fixture_env(request),
        source_profile: "claude-isolated-no-tools-no-fork-v1; managed settings may apply; unsupported startup must refuse",
    })
}

fn validate_request(request: &ClaudeFixtureRequest) -> Result<(), FixtureRequestError> {
    let paths = [
        &request.logical_executable,
        &request.home,
        &request.config,
        &request.xdg_config,
        &request.xdg_cache,
        &request.temporary,
        &request.claude_temporary,
    ];
    if paths.iter().any(|path| !valid_absolute_path(path)) {
        return Err(FixtureRequestError::InvalidPrivatePath);
    }
    if request.port == 0
        || !(16..=64).contains(&request.nonce.len())
        || !request
            .nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(FixtureRequestError::InvalidEndpoint);
    }
    Ok(())
}

fn valid_absolute_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() > 1
        && path.len() <= 4096
        && !path.chars().any(char::is_control)
        && !path.split('/').any(|part| part == "." || part == "..")
}

fn fixture_args(nonce: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "--bare",
        "--safe-mode",
        "--restricted",
        "--disable-slash-commands",
        "--setting-sources",
        "",
        "--strict-mcp-config",
        "--mcp-config",
        "{\"mcpServers\":{}}",
        "--tools",
        "",
        "--permission-mode",
        "dontAsk",
        "--permission-prompts",
        "none",
        "--no-chrome",
        "--no-session-persistence",
        "--print",
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        "sonnet",
        "--max-budget-usd",
        "0.01",
        "--system-prompt",
        FIXTURE_INSTRUCTIONS,
        "--",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    args.push(format!(
        "Harflow fixture nonce {nonce}. Reply with HARFLOW_FIXTURE_OK:{nonce}."
    ));
    args
}

fn fixture_env(request: &ClaudeFixtureRequest) -> Vec<(String, String)> {
    [
        ("HOME", request.home.clone()),
        ("CLAUDE_CONFIG_DIR", request.config.clone()),
        ("XDG_CONFIG_HOME", request.xdg_config.clone()),
        ("XDG_CACHE_HOME", request.xdg_cache.clone()),
        ("TMPDIR", request.temporary.clone()),
        ("CLAUDE_CODE_TMPDIR", request.claude_temporary.clone()),
        ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin".into()),
        ("LANG", "C".into()),
        ("LC_ALL", "C".into()),
        ("SHELL", "/bin/sh".into()),
        ("ANTHROPIC_API_KEY", "harflow-fake-fixture-key".into()),
        (
            "ANTHROPIC_BASE_URL",
            format!("http://127.0.0.1:{}/{}/", request.port, request.nonce),
        ),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1".into()),
        ("DISABLE_AUTOUPDATER", "1".into()),
        ("DISABLE_TELEMETRY", "1".into()),
        ("DISABLE_ERROR_REPORTING", "1".into()),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value))
    .collect()
}

/// Every stock strict production route remains unsupported, including Claude.
/// A launcher hash (notably Vibe's) cannot establish full runtime identity.
pub fn strict_route_unsupported(backend: &str) -> StrictRouteUnsupported {
    StrictRouteUnsupported {
        backend: backend.into(),
        reason: "Strict production execution is unsupported: qualify exact runtime closure, caller and grant authority, stable topology, OS enforcement, containment and budgets; diagnostic fixtures do not grant admission.".into(),
    }
}
