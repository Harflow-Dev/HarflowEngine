use super::*;
impl<P: OwnerPort> OwnerSession<P> {
    pub fn begin_capture(
        &mut self,
        binding: &CaptureBinding,
        deadline: Instant,
    ) -> Result<(), OwnerError<P::Error>> {
        self.healthy()?;
        let Some(expected) = self.state.capture_start(binding) else {
            return self.refuse(OwnerError::InvalidState);
        };
        let result = self.port.capture_begin(binding, deadline);
        let actual = self.response(result)?;
        self.save_capture(actual, expected)
    }

    fn save_capture(
        &mut self,
        actual: CaptureSnapshot,
        expected: CaptureSnapshot,
    ) -> Result<(), OwnerError<P::Error>> {
        if !self.state.accept_capture(actual, expected) {
            return Err(OwnerError::InvalidResponse);
        }
        Ok(())
    }

    fn active_capture(&mut self) -> Result<CaptureSnapshot, OwnerError<P::Error>> {
        self.healthy()?;
        match self.state.capture.as_ref() {
            Some(value) if value.exit == ProcessExit::Unobserved => Ok(value.clone()),
            _ => self.refuse(OwnerError::InvalidState),
        }
    }
    pub fn publish_capture(
        &mut self,
        next: &CaptureProgress,
        deadline: Instant,
    ) -> Result<(), OwnerError<P::Error>> {
        let prior = self.active_capture()?;
        let Some(expected) = self.state.capture_next(next) else {
            return self.refuse(OwnerError::InvalidState);
        };
        let result =
            self.port
                .capture_publish(&prior.binding, prior.progress.revision, next, deadline);
        let actual = self.response(result)?;
        self.save_capture(actual, expected)
    }
    pub fn finish_capture(
        &mut self,
        exit: ProcessExit,
        deadline: Instant,
    ) -> Result<(), OwnerError<P::Error>> {
        let prior = self.active_capture()?;
        let Some(expected) = self.state.capture_terminal(exit) else {
            return self.refuse(OwnerError::InvalidState);
        };
        let result =
            self.port
                .capture_finish(&prior.binding, prior.progress.revision, exit, deadline);
        let actual = self.response(result)?;
        self.save_capture(actual, expected)
    }
}
pub(crate) fn valid_progress(prior: &CaptureProgress, next: &CaptureProgress) -> bool {
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
