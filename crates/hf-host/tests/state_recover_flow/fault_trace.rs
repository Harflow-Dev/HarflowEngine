use super::*;
use hf_platform::ctl::archive_fs::ArchiveFaultPort;
use std::{
    cell::{Cell, RefCell},
    io,
};
pub struct Fault {
    pub at: usize,
    pub calls: RefCell<Vec<&'static str>>,
    pub abort: bool,
    pub root: PathBuf,
    pub marker_first: Cell<bool>,
    pub marker_moved: Cell<bool>,
}
impl Fault {
    pub fn new(at: usize, root: &Path) -> Self {
        Self {
            at,
            calls: RefCell::new(Vec::new()),
            abort: false,
            root: root.into(),
            marker_first: Cell::new(false),
            marker_moved: Cell::new(false),
        }
    }
}
impl ArchiveFaultPort for Fault {
    fn before(&self, boundary: &'static str) -> io::Result<()> {
        let mut calls = self.calls.borrow_mut();
        if boundary == "entry.rename" && !self.marker_first.replace(true) {
            assert!(
                self.root.join("store.identity").exists(),
                "marker must be first"
            );
        }
        if boundary == "entry.root_sync" && !self.marker_moved.replace(true) {
            assert!(
                !self.root.join("store.identity").exists(),
                "first rename must move marker"
            );
            for name in ["nested", "link", "store.sqlite3", "store.fence"] {
                assert!(
                    fs::symlink_metadata(self.root.join(name)).is_ok(),
                    "marker must precede {name}"
                );
            }
        }
        calls.push(boundary);
        if calls.len() == self.at {
            if self.abort {
                std::process::abort();
            }
            return Err(io::Error::other("fixed recovery fault"));
        }
        Ok(())
    }
}
