use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
#[derive(Clone, Debug)]
pub struct SpawnSpec {
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub args: Vec<OsString>,
    pub environment: Vec<(OsString, OsString)>,
    pub capture_limit: usize,
}
impl SpawnSpec {
    pub(super) fn validate(&self) -> io::Result<()> {
        if self.args.len() > 128
            || self.environment.len() > 128
            || self.executable.as_os_str().len() > 4096
            || self.cwd.as_os_str().len() > 4096
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "spawn input count/path exceeds bound",
            ));
        }
        let bytes = self
            .args
            .iter()
            .map(|v| v.len())
            .chain(
                self.environment
                    .iter()
                    .flat_map(|(k, v)| [k.len(), v.len()]),
            )
            .try_fold(0_usize, |sum, len| sum.checked_add(len))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "spawn size overflow"))?;
        if !self.executable.is_absolute()
            || !self.cwd.is_absolute()
            || self.args.len() > 128
            || self.environment.len() > 128
            || bytes > 65536
            || self.capture_limit > 1_048_576
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid bounded spawn specification",
            ));
        }
        Ok(())
    }
}
