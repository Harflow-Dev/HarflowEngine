use std::{os::unix::fs::MetadataExt, path::Path, process::Command};

pub(crate) fn host_facts() -> String {
    format!(
        "{{\"os\":{},\"build\":{},\"architecture\":{},\"boot_uuid\":{}}}",
        json_command("/usr/bin/sw_vers", &["-productVersion"]),
        json_command("/usr/bin/sw_vers", &["-buildVersion"]),
        json_command("/usr/bin/uname", &["-m"]),
        json_command("/usr/sbin/sysctl", &["-n", "kern.uuid"]),
    )
}

fn json_command(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .env_clear()
        .output()
        .unwrap_or_else(|error| panic!("run fixed host observer {program}: {error}"));
    assert!(output.status.success(), "host observer failed: {program}");
    crate::support::json_string(
        std::str::from_utf8(trim_line(&output.stdout)).expect("host observer UTF-8"),
    )
}

pub(crate) fn file_identity(path: &Path) -> (u64, u64, u64) {
    let meta = std::fs::symlink_metadata(path).expect("read artifact identity");
    (meta.dev(), meta.ino(), meta.len())
}

fn trim_line(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}
