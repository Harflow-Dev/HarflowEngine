use super::*;
use hf_platform::qualification::live::ObserverFile;
pub(super) struct Sources {
    files: Vec<(canary::Canary, String)>,
    pub(super) version: ObserverFile,
    pub(super) negative: ObserverFile,
    pub(super) version_path: std::path::PathBuf,
    pub(super) negative_path: std::path::PathBuf,
}
impl Sources {
    pub(super) fn create(root: &FixtureRoot) -> Self {
        let mut files = Vec::new();
        let mut total = 0;
        for (name, bytes) in [
            (
                "client_reason_preload.cjs",
                include_bytes!("client_reason_preload.cjs").as_slice(),
            ),
            (
                "client_reason_classifier.cjs",
                include_bytes!("client_reason_classifier.cjs").as_slice(),
            ),
            (
                "client_reason_hook.cjs",
                include_bytes!("client_reason_hook.cjs").as_slice(),
            ),
            (
                "client_reason_writer.cjs",
                include_bytes!("client_reason_writer.cjs").as_slice(),
            ),
        ] {
            total += bytes.len();
            let file = canary::Canary::create(root.protected().join(name), bytes);
            let hash = digest(file.path());
            eprintln!("phase=client-reason-source hash={hash}");
            files.push((file, hash));
        }
        assert!(total <= 16384, "fixed aggregate source budget");
        let version_path = root.runtime().join("client-reason-version");
        let negative_path = root.runtime().join("client-reason-negative");
        Self {
            files,
            version: ObserverFile::create(&version_path).unwrap(),
            negative: ObserverFile::create(&negative_path).unwrap(),
            version_path,
            negative_path,
        }
    }
    pub(super) fn verify(&self) -> bool {
        self.files
            .iter()
            .map(|(file, hash)| {
                super::client_reason::caught(|| {
                    file.verify();
                    assert!(digest(file.path()) == *hash, "fixed source hash bookend");
                })
                .is_ok()
            })
            .fold(true, |all, valid| all & valid)
    }
    pub(super) fn environment(
        &self,
        attempt: &Attempt,
        version: bool,
    ) -> Vec<(OsString, OsString)> {
        let (file, path) = if version {
            (&self.version, &self.version_path)
        } else {
            (&self.negative, &self.negative_path)
        };
        assert!(
            self.files[0].0.path().to_str().is_some_and(|p| p
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))),
            "fixed preload path"
        );
        let mut env = environment(&attempt.command().env);
        env.retain(|(key, _)| key != "BUN_OPTIONS");
        for (key, value) in [
            (
                "BUN_OPTIONS",
                format!("--preload={}", self.files[0].0.path().display()),
            ),
            (
                "HARFLOW_TLS_OBSERVER_FILE",
                path.to_string_lossy().into_owned(),
            ),
            ("HARFLOW_TLS_OBSERVER_NONCE", attempt.nonce.clone()),
            ("HARFLOW_TLS_OBSERVER_DEV", file.device().to_string()),
            ("HARFLOW_TLS_OBSERVER_INODE", file.inode().to_string()),
        ] {
            env.push((key.into(), value.into()));
        }
        env
    }
}
fn digest(path: &Path) -> String {
    let mut hash = hf_storage::Sha256Stream::with_limit(16384).unwrap();
    hash.update(&fs::read(path).unwrap()).unwrap();
    hash.finish().unwrap().as_str().to_owned()
}
