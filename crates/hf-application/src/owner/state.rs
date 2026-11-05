use super::*;
impl SharedOwnerState {
    pub(crate) fn capture_start(&self, b: &CaptureBinding) -> Option<CaptureSnapshot> {
        let s = self.snapshot.as_ref()?;
        if self.poisoned
            || self.capture.is_some()
            || s.phase != Phase::AcceptanceRecorded
            || s.fence().as_ref() != Some(&b.fence)
            || s.child.as_ref() != Some(&b.child)
            || b.stdout == b.stderr
            || b.stdout.inode == 0
            || b.stderr.inode == 0
        {
            return None;
        }
        Some(CaptureSnapshot {
            binding: b.clone(),
            progress: CaptureProgress {
                revision: 0,
                stdout: 0,
                stderr: 0,
                stdout_eof: false,
                stderr_eof: false,
                loss: false,
            },
            exit: ProcessExit::Unobserved,
        })
    }
    pub(crate) fn capture_next(&self, next: &CaptureProgress) -> Option<CaptureSnapshot> {
        let mut c = self.capture.clone()?;
        if self.poisoned
            || c.exit != ProcessExit::Unobserved
            || !capture::valid_progress(&c.progress, next)
        {
            return None;
        }
        c.progress = next.clone();
        Some(c)
    }
    pub(crate) fn capture_terminal(&self, exit: ProcessExit) -> Option<CaptureSnapshot> {
        let mut c = self.capture.clone()?;
        if self.poisoned
            || c.exit != ProcessExit::Unobserved
            || !c.progress.stdout_eof
            || !c.progress.stderr_eof
            || c.progress.loss
            || c.progress.revision >= 128
            || !matches!(exit, ProcessExit::Code(_) | ProcessExit::Signal(1..))
        {
            return None;
        }
        c.progress.revision += 1;
        c.exit = exit;
        Some(c)
    }
    pub(crate) fn next_snapshot(
        &self,
        command: &hf_domain::owner_runtime::StorageCommand,
    ) -> Option<Snapshot> {
        use hf_domain::owner_runtime::StorageCommand;
        if self.poisoned || self.capture.is_some() {
            return None;
        }
        let mut s = self.snapshot.clone().unwrap_or_else(|| self.prepared());
        match command {
            StorageCommand::Claim if s.phase == Phase::Prepared && s.revision == 0 => {
                s.phase = Phase::Claimed;
                s.owner = Some(self.owner.clone());
            }
            StorageCommand::RecordChild(child)
                if s.phase == Phase::Claimed && child != &self.owner.process =>
            {
                s.phase = Phase::SpawnObserved;
                s.child = Some(child.clone());
            }
            StorageCommand::Accept if s.phase == Phase::SpawnObserved => {
                s.phase = Phase::AcceptanceRecorded
            }
            _ => return None,
        }
        s.revision = s.revision.checked_add(1)?;
        Some(s)
    }
}
