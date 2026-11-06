use super::*;
pub(super) fn prepare_dirs(root: &FixtureRoot) {
    let private = root.runtime().join("claude-private");
    fs::create_dir(&private).unwrap();
    for name in [
        "home",
        "config",
        "xdg-config",
        "xdg-cache",
        "tmp",
        "claude-tmp",
    ] {
        fs::create_dir(private.join(name)).unwrap();
    }
}
pub(super) fn request(
    root: &FixtureRoot,
    nonce: &str,
    port: u16,
    executable: &Path,
) -> ClaudeFixtureRequest {
    let private = root.runtime().join("claude-private");
    let paths = [
        "home",
        "config",
        "xdg-config",
        "xdg-cache",
        "tmp",
        "claude-tmp",
    ]
    .map(|n| private.join(n));
    let p = paths.map(|path| path.to_string_lossy().into_owned());
    ClaudeFixtureRequest {
        logical_executable: executable.to_string_lossy().into_owned(),
        home: p[0].clone(),
        config: p[1].clone(),
        xdg_config: p[2].clone(),
        xdg_cache: p[3].clone(),
        temporary: p[4].clone(),
        claude_temporary: p[5].clone(),
        port,
        nonce: nonce.into(),
    }
}
pub(super) fn environment(pairs: &[(String, String)]) -> Vec<(OsString, OsString)> {
    let mut env: Vec<_> = pairs.iter().map(|(k, v)| (k.into(), v.into())).collect();
    for (key, value) in [
        ("ICU_TIMEZONE_FILES_DIR", ""),
        ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
        ("LANG", "C"),
        ("LC_ALL", "C"),
        ("PYTHONNOUSERSITE", "1"),
        ("PYTHONDONTWRITEBYTECODE", "1"),
    ] {
        if !env.iter().any(|(present, _)| present == key) {
            env.push((key.into(), value.into()));
        }
    }
    env
}
pub(super) fn certificate_error(stdout: &[u8], stderr: &[u8]) -> bool {
    [stdout, stderr].iter().any(|raw| {
        [
            b"CERTIFICATE_VERIFY_FAILED".as_slice(),
            b"self-signed certificate",
            b"unable to verify",
        ]
        .iter()
        .any(|needle| {
            raw.windows(needle.len())
                .any(|window| window.eq_ignore_ascii_case(needle))
        })
    })
}
