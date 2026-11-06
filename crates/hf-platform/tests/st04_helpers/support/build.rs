use hf_platform::st04_fixture::{BUILD_ARGUMENTS, HELPER_SOURCE};
use std::{
    fs,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(crate) struct Build {
    pub(crate) source: PathBuf,
    pub(crate) binary: PathBuf,
    pub(crate) compiler: PathBuf,
    pub(crate) arguments: Vec<String>,
    pub(crate) source_hash: String,
    pub(crate) binary_hash: String,
    pub(crate) compiler_hash: String,
    pub(crate) compiler_target: PathBuf,
    pub(crate) compiler_identity: (u64, u64),
}

impl Build {
    pub(crate) fn compile(root: &Path, evidence: &Path) -> Self {
        let source = root.join("workspace/st04-fixed-helper.c");
        let binary = root.join("workspace/st04-fixed-helper");
        fs::write(&source, HELPER_SOURCE).expect("write pinned native helper source");
        let compiler = PathBuf::from("/usr/bin/clang");
        let compiler_target = fs::canonicalize(&compiler).expect("resolve compiler target");
        let compiler_meta = fs::metadata(&compiler_target).expect("compiler target metadata");
        let mut args: Vec<String> = BUILD_ARGUMENTS.iter().map(|item| (*item).into()).collect();
        args.extend(["-o".into(), binary.to_string_lossy().into_owned()]);
        args.push(source.to_string_lossy().into_owned());
        let output = Command::new(&compiler)
            .args(&args)
            .env_clear()
            .output()
            .expect("run fixed absolute clang compiler");
        assert!(
            output.status.success(),
            "pinned helper compilation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(&source).expect("read helper source"),
            HELPER_SOURCE.as_bytes()
        );
        super::write_bytes(&evidence.join("helper.c"), &read_bounded(&source));
        super::write_bytes(&evidence.join("helper"), &read_bounded(&binary));
        Self {
            source_hash: sha256(&source),
            binary_hash: sha256(&binary),
            compiler_hash: compiler_digest(&compiler_target, evidence),
            compiler_identity: (compiler_meta.dev(), compiler_meta.ino()),
            source,
            binary,
            compiler,
            arguments: args,
            compiler_target,
        }
    }
}

pub(crate) fn sha256(path: &Path) -> String {
    let before = fs::symlink_metadata(path).expect("hash input metadata");
    assert!(before.is_file() && !before.file_type().is_symlink() && before.nlink() == 1);
    assert!(before.len() <= 16 * 1024 * 1024, "hash input bound");
    let input = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x100) // Darwin O_NOFOLLOW
        .open(path)
        .expect("open physical hash input");
    let output = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .stdin(Stdio::from(input))
        .env_clear()
        .output()
        .expect("run fixed SHA-256 observer");
    assert!(output.status.success(), "SHA-256 observer failed");
    assert!(output.stdout.len() <= 256 && output.stderr.len() <= 4096);
    let after = fs::symlink_metadata(path).expect("hash input post-read metadata");
    assert_eq!(
        (before.dev(), before.ino(), before.len()),
        (after.dev(), after.ino(), after.len())
    );
    String::from_utf8(output.stdout)
        .expect("SHA-256 output UTF-8")
        .split_whitespace()
        .next()
        .expect("SHA-256 digest")
        .to_owned()
}

fn read_bounded(path: &Path) -> Vec<u8> {
    use std::io::Read;
    let before = fs::symlink_metadata(path).expect("artifact metadata");
    assert!(before.is_file() && !before.file_type().is_symlink() && before.nlink() == 1);
    assert!(before.len() <= 16 * 1024 * 1024);
    let mut bytes = Vec::new();
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x100)
        .open(path)
        .expect("open physical artifact")
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .expect("read bounded artifact");
    let after = fs::symlink_metadata(path).expect("artifact post-read metadata");
    assert_eq!(
        (before.dev(), before.ino(), before.len()),
        (after.dev(), after.ino(), after.len())
    );
    assert_eq!(bytes.len() as u64, after.len());
    bytes
}

// Apple tool shims may have multiple links. Snapshot this exact read-only system
// input; fixture/source/helper hash inputs retain their single-link requirement.
fn compiler_digest(path: &Path, evidence: &Path) -> String {
    use std::io::{Read, Write};
    assert_eq!(path, Path::new("/usr/bin/clang"));
    let before = fs::symlink_metadata(path).expect("system compiler identity");
    assert!(before.is_file() && !before.file_type().is_symlink());
    assert!(before.len() <= 16 * 1024 * 1024);
    let mut input = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x100)
        .open(path)
        .expect("open system compiler without following links");
    let opened = input.metadata().expect("opened compiler identity");
    assert_eq!((before.dev(), before.ino()), (opened.dev(), opened.ino()));
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut input)
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .expect("bounded compiler snapshot");
    assert_eq!(bytes.len() as u64, before.len());
    let after = fs::symlink_metadata(path).expect("compiler bookend");
    assert_eq!(
        (
            before.dev(),
            before.ino(),
            before.len(),
            before.mtime(),
            before.mtime_nsec()
        ),
        (
            after.dev(),
            after.ino(),
            after.len(),
            after.mtime(),
            after.mtime_nsec()
        )
    );
    let snapshot = evidence.join("compiler-snapshot");
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&snapshot)
        .expect("create compiler snapshot");
    output.write_all(&bytes).expect("record compiler bytes");
    output.sync_all().expect("sync compiler snapshot");
    sha256(&snapshot)
}
