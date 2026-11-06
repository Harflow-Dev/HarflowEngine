use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) fn evidence_dir() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let base = Path::new("/private/tmp").join(format!(
        "harflow-st04-helper-evidence-{}",
        std::process::id()
    ));
    if !base.exists() {
        DirBuilder::new()
            .mode(0o700)
            .create(&base)
            .expect("create external ST04 evidence directory");
        return base;
    }
    loop {
        let candidate = base.join(format!("case-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
        match DirBuilder::new().mode(0o700).create(&candidate) {
            Ok(()) => return candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create evidence case directory: {error}"),
        }
    }
}

pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) {
    use std::io::Write;
    let mut file =
        fs::File::create(path).unwrap_or_else(|error| panic!("create {}: {error}", path.display()));
    file.write_all(bytes)
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    file.sync_all()
        .unwrap_or_else(|error| panic!("sync {}: {error}", path.display()));
}
