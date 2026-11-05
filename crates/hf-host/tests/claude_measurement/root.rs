#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_platform::qualification::FixtureRoot;
use std::fs;

pub(super) struct SettledRoot {
    pub(super) fixture: FixtureRoot,
    settled: bool,
}
impl SettledRoot {
    pub(super) fn create() -> Self {
        Self {
            fixture: FixtureRoot::create().expect("fresh qualification root"),
            settled: false,
        }
    }
    pub(super) fn settle(&mut self) {
        self.settled = true;
    }
}
impl Drop for SettledRoot {
    fn drop(&mut self) {
        if !self.settled {
            eprintln!(
                "retaining unsettled Claude measurement root: {}",
                self.fixture.root().display()
            );
            return;
        }
        fs::remove_dir_all(self.fixture.root()).expect("remove settled harness-owned root");
    }
}
