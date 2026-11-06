use std::{
    env, fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
pub(super) struct VendorSelection {
    pub(super) logical: PathBuf,
    pub(super) expected_hash: String,
}
pub(super) fn nonce_from_root(root: &Path) -> String {
    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .expect("fixture directory name is UTF-8");
    let nonce = name
        .strip_prefix("harflow-os-measurement-")
        .expect("fixture root has the harness prefix");
    assert!(
        nonce.len() == 32 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "fixture nonce is the random directory suffix"
    );
    nonce.to_owned()
}

pub(super) fn selection() -> VendorSelection {
    assert_eq!(
        env::var("HARFLOW_CLAUDE_EXPECTED_VERSION").as_deref(),
        Ok("2.1.318")
    );
    let logical =
        PathBuf::from(env::var("HARFLOW_CLAUDE_LOGICAL_EXECUTABLE").expect("explicit executable"));
    assert_eq!(
        logical,
        Path::new("/Users/nayanda/.local/share/claude/versions/2.1.318")
    );
    let expected_hash = env::var("HARFLOW_CLAUDE_EXPECTED_SHA256").expect("explicit hash");
    VendorSelection {
        logical,
        expected_hash,
    }
}
pub(super) fn same_metadata(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
        && left.nlink() == right.nlink()
}
