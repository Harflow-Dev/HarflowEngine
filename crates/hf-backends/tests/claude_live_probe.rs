#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_backends::{claude_live_probe::live_command, claude_measurement::ClaudeFixtureRequest};

fn request() -> ClaudeFixtureRequest {
    ClaudeFixtureRequest {
        logical_executable: "/opt/homebrew/bin/claude".into(),
        home: "/tmp/owned/home".into(),
        config: "/tmp/owned/config".into(),
        xdg_config: "/tmp/owned/xdg-config".into(),
        xdg_cache: "/tmp/owned/xdg-cache".into(),
        temporary: "/tmp/owned/tmp".into(),
        claude_temporary: "/tmp/owned/claude-tmp".into(),
        port: 42123,
        nonce: "a1b2c3d4e5f60718".into(),
    }
}

#[test]
fn live_command_is_oauth_only_no_tools_and_proxy_bound_to_request_port() {
    let req = request();
    let command = live_command(&req, req.port).unwrap();
    assert_eq!(command.executable, req.logical_executable);
    assert!(!command.args.iter().any(|arg| arg == "--bare"));
    assert!(
        command
            .args
            .windows(2)
            .any(|a| a[0] == "--max-turns" && a[1] == "1")
    );
    assert!(
        command
            .args
            .windows(2)
            .any(|a| a[0] == "--tools" && a[1].is_empty())
    );
    assert!(command.args.iter().any(|arg| arg == "--strict-mcp-config"));
    assert!(command.args.iter().any(|arg| arg == "--permission-mode"));
    assert!(command.args.iter().any(|arg| arg == "dontAsk"));
    assert!(command.args.iter().any(|arg| arg == "none"));
    assert!(command.args.last().unwrap().contains(&req.nonce));
    assert!(
        !command
            .env
            .iter()
            .any(|(key, _)| { key == "ANTHROPIC_API_KEY" || key == "ANTHROPIC_BASE_URL" })
    );
    for key in ["HTTP_PROXY", "HTTPS_PROXY"] {
        assert!(command.env.iter().any(|(name, value)| {
            name == key && value == &format!("http://127.0.0.1:{}", req.port)
        }));
    }
    assert!(!command.env.iter().any(|(key, value)| {
        key.to_ascii_lowercase().contains("proxy")
            && !matches!(key.as_str(), "HTTP_PROXY" | "HTTPS_PROXY")
            && !value.starts_with("http://127.0.0.1:")
    }));
    assert!(
        command
            .env
            .iter()
            .all(|(key, value)| { key != "CLAUDE_CODE_OAUTH_TOKEN" && !value.contains("fake") })
    );
}

#[test]
fn live_command_rejects_wrong_or_empty_proxy_port() {
    let req = request();
    assert!(live_command(&req, 0).is_err());
    assert!(live_command(&req, req.port + 1).is_err());
}
