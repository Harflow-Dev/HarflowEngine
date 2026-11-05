#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_backends::claude_measurement::{
    ClaudeFixtureRequest, FixtureRequestError, claude_fixture_command, strict_route_unsupported,
};

fn request() -> ClaudeFixtureRequest {
    ClaudeFixtureRequest {
        logical_executable: "/opt/homebrew/bin/claude".into(),
        home: "/tmp/fixture/home".into(),
        config: "/tmp/fixture/config".into(),
        xdg_config: "/tmp/fixture/xdg-config".into(),
        xdg_cache: "/tmp/fixture/xdg-cache".into(),
        temporary: "/tmp/fixture/tmp".into(),
        claude_temporary: "/tmp/fixture/claude-tmp".into(),
        port: 42123,
        nonce: "a1b2c3d4e5f60718".into(),
    }
}

#[test]
fn fixture_command_preserves_logical_executable_and_isolates_settings_and_provider() {
    let req = request();
    let command = claude_fixture_command(&req).unwrap();
    assert_eq!(command.executable, req.logical_executable);
    assert_eq!(
        command.source_profile,
        "claude-isolated-no-tools-no-fork-v1; managed settings may apply; unsupported startup must refuse"
    );
    assert!(command.args.iter().any(|arg| arg == "--tools"));
    assert!(command.args.iter().any(String::is_empty));
    assert!(command.args.last().unwrap().contains(&req.nonce));
    assert!(
        command
            .env
            .iter()
            .any(|(key, _)| key == "ANTHROPIC_API_KEY")
    );
    assert!(
        command
            .env
            .iter()
            .any(|(key, value)| key == "ANTHROPIC_BASE_URL"
                && value.ends_with(&format!("/{}/", req.nonce)))
    );
    assert!(!command.env.iter().any(|(key, _)| key.contains("PROXY")));
}

#[test]
fn factory_refuses_invalid_private_paths_and_endpoint_values() {
    let mut req = request();
    req.home = "/tmp/../user-home".into();
    assert_eq!(
        claude_fixture_command(&req),
        Err(FixtureRequestError::InvalidPrivatePath)
    );
    let mut req = request();
    req.port = 0;
    assert_eq!(
        claude_fixture_command(&req),
        Err(FixtureRequestError::InvalidEndpoint)
    );
    let mut req = request();
    req.nonce = "short".into();
    assert_eq!(
        claude_fixture_command(&req),
        Err(FixtureRequestError::InvalidEndpoint)
    );
}

#[test]
fn every_stock_strict_route_stays_unsupported() {
    for backend in ["claude", "codex", "vibe", "opencode", "agy"] {
        let refusal = strict_route_unsupported(backend);
        assert_eq!(refusal.backend, backend);
        assert!(refusal.reason.contains("unsupported"));
        assert!(refusal.reason.contains("OS enforcement"));
    }
}
