use super::*;
use hf_backends::claude_measurement::{ClaudeFixtureRequest, claude_fixture_command};

const TIMEZONE_KEY: &str = "ICU_TIMEZONE_FILES_DIR";

fn request() -> ClaudeFixtureRequest {
    ClaudeFixtureRequest {
        logical_executable: "/vendor/claude".into(),
        home: "/private/home".into(),
        config: "/private/config".into(),
        xdg_config: "/private/xdg-config".into(),
        xdg_cache: "/private/xdg-cache".into(),
        temporary: "/private/tmp".into(),
        claude_temporary: "/private/claude-tmp".into(),
        port: 43123,
        nonce: "environmenttestnonce1234".into(),
    }
}

#[test]
fn bundled_timezone_adds_only_the_empty_override_to_the_protocol_command() {
    let base = claude_fixture_command(&request()).unwrap();
    let configured = with_bundled_timezone(base.clone());

    assert_eq!(configured.executable, base.executable);
    assert_eq!(configured.args, base.args);
    assert_eq!(configured.source_profile, base.source_profile);
    assert_eq!(configured.env.len(), base.env.len() + 1);
    assert_eq!(&configured.env[..base.env.len()], base.env.as_slice());
    assert_eq!(
        configured.env.last().unwrap(),
        &(TIMEZONE_KEY.into(), String::new())
    );
    assert_eq!(
        configured
            .env
            .iter()
            .filter(|(key, _)| key == TIMEZONE_KEY)
            .count(),
        1
    );
}

#[test]
#[should_panic(expected = "protocol timezone configuration must be explicit and unique")]
fn bundled_timezone_refuses_a_preexisting_key() {
    let mut command = claude_fixture_command(&request()).unwrap();
    command
        .env
        .push((TIMEZONE_KEY.into(), "already-set".into()));
    let _ = with_bundled_timezone(command);
}
