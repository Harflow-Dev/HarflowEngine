use super::*;
impl<P: OwnerPort> OwnerSession<P> {
    pub fn begin_capture(
        &mut self,
        binding: &CaptureBinding,
        deadline: Instant,
    ) -> Result<(), OwnerError<P::Error>> {
        let owner = self.current(Phase::AcceptanceRecorded)?;
        if owner.fence().as_ref() != Some(&binding.fence)
            || owner.child.as_ref() != Some(&binding.child)
            || binding.stdout == binding.stderr
            || binding.stdout.inode == 0
            || binding.stderr.inode == 0
        {
            return self.refuse(OwnerError::InvalidState);
        }
        let result = self.port.capture_begin(binding, deadline);
        let actual = self.response(result)?;
        self.save_capture(
            actual,
            CaptureSnapshot {
                binding: binding.clone(),
                progress: CaptureProgress {
                    revision: 0,
                    stdout: 0,
                    stderr: 0,
                    stdout_eof: false,
                    stderr_eof: false,
                    loss: false,
                },
                exit: ProcessExit::Unobserved,
            },
        )
    }
    fn save_capture(
        &mut self,
        actual: CaptureSnapshot,
        expected: CaptureSnapshot,
    ) -> Result<(), OwnerError<P::Error>> {
        if actual != expected {
            return self.refuse(OwnerError::InvalidResponse);
        }
        self.capture = Some(actual);
        Ok(())
    }
    fn active_capture(&mut self) -> Result<CaptureSnapshot, OwnerError<P::Error>> {
        self.healthy()?;
        match self.capture.as_ref() {
            Some(value) if value.exit == ProcessExit::Unobserved => Ok(value.clone()),
            _ => self.refuse(OwnerError::InvalidState),
        }
    }
    pub fn publish_capture(
        &mut self,
        next: &CaptureProgress,
        deadline: Instant,
    ) -> Result<(), OwnerError<P::Error>> {
        let mut expected = self.active_capture()?;
        if !valid_progress(&expected.progress, next) {
            return self.refuse(OwnerError::InvalidState);
        }
        let result = self.port.capture_publish(
            &expected.binding,
            expected.progress.revision,
            next,
            deadline,
        );
        let actual = self.response(result)?;
        expected.progress = next.clone();
        self.save_capture(actual, expected)
    }
    pub fn finish_capture(
        &mut self,
        exit: ProcessExit,
        deadline: Instant,
    ) -> Result<(), OwnerError<P::Error>> {
        let mut expected = self.active_capture()?;
        let p = &expected.progress;
        if !p.stdout_eof
            || !p.stderr_eof
            || p.loss
            || p.revision >= 128
            || !matches!(exit, ProcessExit::Code(_) | ProcessExit::Signal(1..))
        {
            return self.refuse(OwnerError::InvalidState);
        }
        let result = self
            .port
            .capture_finish(&expected.binding, p.revision, exit, deadline);
        let actual = self.response(result)?;
        expected.progress.revision += 1;
        expected.exit = exit;
        self.save_capture(actual, expected)
    }
}
fn valid_progress(prior: &CaptureProgress, next: &CaptureProgress) -> bool {
    prior.revision < 128
        && next.revision == prior.revision + 1
        && next.stdout <= 65536
        && next.stderr <= 65536
        && next.stdout >= prior.stdout
        && next.stderr >= prior.stderr
        && next.stdout - prior.stdout <= 16384
        && next.stderr - prior.stderr <= 16384
        && (!prior.stdout_eof || (next.stdout_eof && next.stdout == prior.stdout))
        && (!prior.stderr_eof || (next.stderr_eof && next.stderr == prior.stderr))
        && (!prior.loss || next.loss)
        && (prior.stdout != next.stdout
            || prior.stderr != next.stderr
            || prior.stdout_eof != next.stdout_eof
            || prior.stderr_eof != next.stderr_eof
            || prior.loss != next.loss)
}
