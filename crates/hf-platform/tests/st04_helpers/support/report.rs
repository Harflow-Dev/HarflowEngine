use super::{Build, file_identity, host_facts, sha256, write_bytes};
use hf_platform::st04_fixture::ProbeControl;
use std::path::Path;

pub(crate) fn json_string(value: &str) -> String {
    let mut result = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            ch if ch.is_control() => result.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => result.push(ch),
        }
    }
    result.push('"');
    result
}

pub(crate) struct Manifest<'a> {
    pub(crate) control: ProbeControl,
    pub(crate) root: &'a Path,
    pub(crate) build: &'a Build,
    pub(crate) profile: &'a str,
    pub(crate) args: &'a [String],
    pub(crate) cwd: &'a Path,
    pub(crate) controller: &'a str,
}

pub(crate) fn write_manifest(directory: &Path, value: Manifest<'_>) -> String {
    let (root_dev, root_ino, _) = file_identity(value.root);
    let profile_path = directory.join("profile.sb");
    super::write_synced_bytes(&profile_path, value.profile.as_bytes());
    let profile_hash = sha256(&profile_path);
    let source_id = file_identity(&value.build.source);
    let helper_id = file_identity(&value.build.binary);
    let controller_path = std::env::current_exe().expect("controller executable");
    let controller_id = file_identity(&controller_path);
    let controller_hash = sha256(&controller_path);
    let args = string_array(value.args.iter().map(String::as_str));
    let compiler_args = string_array(value.build.arguments.iter().map(String::as_str));
    let data = format!(
        concat!(
            "{{\"format\":\"harflow-st04-test-manifest-v1\",\"control\":{},",
            "\"host\":{},\"root\":{{\"path\":{},\"device\":{root_dev},\"inode\":{root_ino}}},",
            "\"source\":{{\"path\":{},\"device\":{},\"inode\":{},\"sha256\":{}}},",
            "\"compiler\":{{\"path\":{},\"physical_path\":{},\"device\":{},\"inode\":{},\"sha256\":{},\"argv\":{}}},",
            "\"helper\":{{\"path\":{},\"device\":{},\"inode\":{},\"sha256\":{}}},",
            "\"profile\":{{\"path\":{},\"revision\":{},\"sha256\":{}}},",
            "\"controller\":{{\"path\":{},\"device\":{},\"inode\":{},\"sha256\":{},\"identity\":{}}},",
            "\"spawn\":{{\"executable\":{},\"cwd\":{},\"argv\":{},\"environment\":[],\"capture_limit\":4096}}}}\n"
        ),
        json_string(&format!("{value_control:?}", value_control = value.control)),
        host_facts(),
        json_string(&value.root.to_string_lossy()),
        json_string(&value.build.source.to_string_lossy()),
        source_id.0,
        source_id.1,
        json_string(&value.build.source_hash),
        json_string(&value.build.compiler.to_string_lossy()),
        json_string(&value.build.compiler_target.to_string_lossy()),
        value.build.compiler_identity.0,
        value.build.compiler_identity.1,
        json_string(&value.build.compiler_hash),
        compiler_args,
        json_string(&value.build.binary.to_string_lossy()),
        helper_id.0,
        helper_id.1,
        json_string(&value.build.binary_hash),
        json_string(&profile_path.to_string_lossy()),
        json_string(hf_platform::st04_fixture::PROFILE_REVISION),
        json_string(&profile_hash),
        json_string(&controller_path.to_string_lossy()),
        controller_id.0,
        controller_id.1,
        json_string(&controller_hash),
        json_string(value.controller),
        json_string("/usr/bin/sandbox-exec"),
        json_string(&value.cwd.to_string_lossy()),
        args,
        root_dev = root_dev,
        root_ino = root_ino,
    );
    let path = directory.join("manifest.json");
    super::write_synced_bytes(&path, data.as_bytes());
    let digest = sha256(&path);
    super::write_synced_bytes(
        &directory.join("manifest.sha256"),
        format!("{digest}\n").as_bytes(),
    );
    digest
}

fn string_array<'a>(values: impl Iterator<Item = &'a str>) -> String {
    format!(
        "[{}]",
        values.map(json_string).collect::<Vec<_>>().join(",")
    )
}

pub(crate) fn write_case_report(directory: &Path, manifest_hash: &str, result: &super::CaseResult) {
    let data = format!(
        concat!(
            "{{\"format\":\"harflow-st04-test-result-v1\",\"manifest_sha256\":{},",
            "\"identity\":{},\"exit\":{},\"stdout_hex\":{},\"stderr_hex\":{},",
            "\"stdout_eof\":{},\"stderr_eof\":{},\"stdout_overflow\":{},\"stderr_overflow\":{},",
            "\"probe_absent\":{},\"probe_device\":{},\"probe_inode\":{},\"probe_len\":{},\"probe_bytes_hex\":{},\"network_bytes_hex\":{},",
            "\"settled\":{},\"absence_qualified\":{},\"assertions\":{}}}\n"
        ),
        json_string(manifest_hash),
        json_string(&result.identity),
        json_string(&result.exit),
        json_string(&hex(&result.stdout)),
        json_string(&hex(&result.stderr)),
        result.stdout_eof,
        result.stderr_eof,
        result.stdout_overflow,
        result.stderr_overflow,
        result.probe_absent,
        optional_number(result.probe_device),
        optional_number(result.probe_inode),
        result.probe_len,
        json_string(&hex(&result.probe_bytes)),
        json_string(&hex(&result.network_bytes)),
        result.settled,
        result.absence_qualified,
        string_array(result.assertions.iter().map(String::as_str)),
    );
    super::write_synced_bytes(&directory.join("result.json"), data.as_bytes());
    write_bytes(directory.join("stdout.bin").as_path(), &result.stdout);
    write_bytes(directory.join("stderr.bin").as_path(), &result.stderr);
    write_bytes(directory.join("probe.bin").as_path(), &result.probe_bytes);
    write_bytes(
        directory.join("network.bin").as_path(),
        &result.network_bytes,
    );
}

fn hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push_str(&format!("{byte:02x}"));
    }
    result
}

fn optional_number(value: Option<u64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "null".into())
}
