#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::{
    env, fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::Instant,
};

const BINARY_HASH_LIMIT: usize = 512 * 1024 * 1024;
const CLAUDE_2_1_318_SHA256: &str =
    "677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d";
const CLAUDE_2_1_318_PATH: &str = "/Users/nayanda/.local/share/claude/versions/2.1.318";

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
    let logical = PathBuf::from(required_env("HARFLOW_CLAUDE_LOGICAL_EXECUTABLE"));
    assert_eq!(required_env("HARFLOW_CLAUDE_EXPECTED_VERSION"), "2.1.318");
    let expected_hash = required_env("HARFLOW_CLAUDE_EXPECTED_SHA256").to_ascii_lowercase();
    assert_eq!(expected_hash, CLAUDE_2_1_318_SHA256);
    assert!(expected_hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let (physical, actual_hash) = stable_binary_hash(&logical);
    assert_eq!(actual_hash, expected_hash);
    assert!(physical.is_absolute());
    VendorSelection {
        logical,
        expected_hash,
    }
}

pub(super) fn qualification_selection() -> VendorSelection {
    VendorSelection {
        logical: PathBuf::from(CLAUDE_2_1_318_PATH),
        expected_hash: CLAUDE_2_1_318_SHA256.to_owned(),
    }
}

fn stable_binary_hash(logical: &Path) -> (PathBuf, String) {
    let physical = fs::canonicalize(logical).expect("selected Claude binary resolves");
    let before = fs::metadata(&physical).expect("selected Claude binary metadata");
    assert!(before.is_file() && before.len() <= BINARY_HASH_LIMIT as u64);
    let mut file = fs::File::open(&physical).expect("open selected Claude target");
    let mut digest = hf_storage::Sha256Stream::with_limit(BINARY_HASH_LIMIT).unwrap();
    let mut chunk = [0_u8; 16 * 1024];
    let mut length = 0_u64;
    let end = Instant::now() + std::time::Duration::from_secs(60);
    loop {
        assert!(Instant::now() < end, "Claude binary hash deadline expired");
        let count = file.read(&mut chunk).expect("bounded Claude binary read");
        if count == 0 {
            break;
        }
        digest.update(&chunk[..count]).unwrap();
        length += count as u64;
    }
    assert_eq!(length, before.len());
    let after = fs::metadata(&physical).expect("Claude binary final metadata");
    assert!(same_metadata(&before, &after));
    (physical, digest.finish().unwrap().as_str().to_owned())
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

fn required_env(name: &str) -> String {
    env::var(name)
        .unwrap_or_else(|_| panic!("set {name} explicitly for the ignored measurement test"))
}
