//! Trusted command derivation for the closed offline diagnostic only.
use crate::claude_measurement::{
    ClaudeFixtureRequest, FixtureRequestError, claude_fixture_command,
};
use hf_domain::claude_qualification::{ClaudeQualification, QualificationCommand, VENDOR_PATH};
pub fn derive_command(
    v: &ClaudeQualification,
) -> Result<QualificationCommand, FixtureRequestError> {
    let root = &v.topology.root.path;
    if root.len() > 4096 || v.nonce.len() > 64 {
        return Err(FixtureRequestError::InvalidPrivatePath);
    }
    let private = format!("{root}/runtime/claude-private");
    let request = ClaudeFixtureRequest {
        logical_executable: VENDOR_PATH.into(),
        home: format!("{private}/home"),
        config: format!("{private}/config"),
        xdg_config: format!("{private}/xdg-config"),
        xdg_cache: format!("{private}/xdg-cache"),
        temporary: format!("{private}/tmp"),
        claude_temporary: format!("{private}/claude-tmp"),
        port: v.port,
        nonce: v.nonce.clone(),
    };
    let mut command = claude_fixture_command(&request)?;
    command
        .env
        .push(("ICU_TIMEZONE_FILES_DIR".into(), String::new()));
    Ok(QualificationCommand {
        executable: command.executable,
        args: command.args,
        environment: command.env,
    })
}
pub fn validate_command(v: &ClaudeQualification) -> Result<(), FixtureRequestError> {
    if v.command.args.len() > 40
        || v.command.environment.len() > 24
        || v.command.executable.len() > 4096
        || v.command.args.iter().any(|s| s.len() > 4096)
        || v.command
            .environment
            .iter()
            .any(|(k, v)| k.len() > 4096 || v.len() > 4096)
    {
        return Err(FixtureRequestError::InvalidPrivatePath);
    }
    if derive_command(v)? != v.command {
        return Err(FixtureRequestError::InvalidPrivatePath);
    }
    Ok(())
}
