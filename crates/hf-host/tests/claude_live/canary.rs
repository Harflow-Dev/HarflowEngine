use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
/// Original creation authority and identity; no automatic failure-path removal.
pub(super) struct Canary {
    path: PathBuf,
    stamp: fs::Metadata,
    bytes: &'static [u8],
}
impl Canary {
    pub(super) fn create(path: PathBuf, bytes: &'static [u8]) -> Self {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .expect("exclusive private control file");
        file.write_all(bytes).unwrap();
        let stamp = file.metadata().unwrap();
        let value = Self { path, stamp, bytes };
        value.verify();
        value
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn verify(&self) {
        let now = fs::symlink_metadata(&self.path).expect("original control identity");
        assert!(now.is_file() && !now.file_type().is_symlink());
        assert!(
            super::identity::same_metadata(&self.stamp, &now),
            "original control metadata changed"
        );
        assert_eq!(fs::read(&self.path).unwrap(), self.bytes);
        let after = fs::symlink_metadata(&self.path).unwrap();
        assert!(super::identity::same_metadata(&self.stamp, &after));
    }
    pub(super) fn remove(self) {
        self.verify();
        fs::remove_file(self.path).expect("remove exact original control file");
    }
}
