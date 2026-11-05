use std::{fs, path::PathBuf};

pub struct Fixture {
    path: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "hf-recovery-platform-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create private fixture");
        Self {
            path: fs::canonicalize(path).expect("canonicalize private fixture"),
        }
    }
    pub fn root(&self) -> PathBuf {
        self.path.join("store")
    }
    pub fn raw_path(&self, stream: &str) -> PathBuf {
        self.path.join(format!(
            "r03-b2c.{}.{}",
            "11111111111111111111111111111111", stream
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
