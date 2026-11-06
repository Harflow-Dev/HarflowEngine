use std::fs::{self, DirBuilder, File, Metadata};
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalIdentity {
    pub device: u64,
    pub inode: u64,
}
impl PhysicalIdentity {
    fn of(meta: &Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
        }
    }
}
/// Created here, never reconstructed from caller assertions. No automatic deletion.
#[derive(Debug)]
pub struct FixtureRoot {
    root: PathBuf,
    identity: PhysicalIdentity,
    pub(super) roles: Vec<(PathBuf, PhysicalIdentity)>,
}
impl FixtureRoot {
    pub fn create() -> io::Result<Self> {
        let parent = fs::canonicalize(std::env::temp_dir())?;
        let mut nonce = [0_u8; 16];
        File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let suffix: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        let root = parent.join(format!("harflow-os-measurement-{suffix}"));
        DirBuilder::new().mode(0o700).create(&root)?;
        let identity = PhysicalIdentity::of(&fs::symlink_metadata(&root)?);
        let mut fixture = Self {
            root,
            identity,
            roles: Vec::with_capacity(6),
        };
        for path in [
            fixture.workspace(),
            fixture.runtime(),
            fixture.workspace().join("nested"),
            fixture.workspace().join(".git"),
            fixture.protected(),
            fixture.outside(),
        ] {
            DirBuilder::new().mode(0o700).create(&path)?;
            let identity = PhysicalIdentity::of(&fs::symlink_metadata(&path)?);
            fixture.roles.push((path, identity));
        }
        fixture.validate_topology()?;
        Ok(fixture)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn workspace(&self) -> PathBuf {
        self.root.join("workspace")
    }
    pub fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }
    pub fn protected(&self) -> PathBuf {
        self.workspace().join("nested/control")
    }
    pub fn outside(&self) -> PathBuf {
        self.root.join("outside")
    }
    pub fn identity(&self) -> &PhysicalIdentity {
        &self.identity
    }
    /// Bounded observations, not an atomic identity or ongoing exclusivity grant.
    pub fn validate_topology(&self) -> io::Result<usize> {
        self.validate_root()?;
        let mut remaining = 1024;
        inventory(&self.root, 0, &mut remaining)?;
        self.validate_root()?;
        Ok(1024 - remaining)
    }
    pub(super) fn validate_root(&self) -> io::Result<()> {
        validate_directory(&self.root, &self.identity)?;
        for (path, identity) in &self.roles {
            validate_directory(path, identity)?;
        }
        Ok(())
    }
}
pub(super) fn validate_directory(path: &Path, identity: &PhysicalIdentity) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.mode() & 0o777 != 0o700
        || PhysicalIdentity::of(&meta) != *identity
        || fs::canonicalize(path)? != path
    {
        return Err(io::Error::other(
            "fixture role changed or is not physical mode 0700",
        ));
    }
    Ok(())
}
pub(super) fn inventory(path: &Path, depth: usize, remaining: &mut usize) -> io::Result<()> {
    if depth > 16 || *remaining == 0 {
        return Err(io::Error::other("fixture topology budget exhausted"));
    }
    *remaining -= 1;
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink()
        || (!meta.is_dir() && !meta.is_file())
        || (meta.is_file() && (meta.nlink() != 1 || meta.len() > 16 * 1024 * 1024))
    {
        return Err(io::Error::other(
            "unqualified alias, special file or file budget",
        ));
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            inventory(&entry?.path(), depth + 1, remaining)?;
        }
    }
    Ok(())
}
