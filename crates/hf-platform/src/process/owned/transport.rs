use super::*;
impl RetainedOwnedChild {
    /// Private fixture transport only; consumes stdout directly without capture
    /// buffering. Do not call `pump` or `wait_until` until the protocol is complete:
    /// those methods also consume stdout. Protocol bytes do not enter capture;
    /// stderr remains separate and must be drained by subsequent capture pumping.
    pub fn read_fixture_frame(&self, deadline: Instant) -> io::Result<Vec<u8>> {
        let stdout = self
            .stdout
            .as_ref()
            .ok_or_else(|| io::Error::other("fixture stdout absent"))?;
        crate::fixture::read_frame(stdout, deadline)
    }
    pub fn write_fixture_frame(&self, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        let input = self
            .stdin
            .as_ref()
            .ok_or_else(|| io::Error::other("fixture stdin absent"))?;
        crate::fixture::write_all(input, bytes, deadline)
    }
    /// Checks protocol EOF without changing capture EOF or exit evidence.
    pub fn fixture_eof(&self, deadline: Instant) -> io::Result<()> {
        let stdout = self
            .stdout
            .as_ref()
            .ok_or_else(|| io::Error::other("fixture stdout absent"))?;
        crate::fixture::require_eof(stdout, deadline)
    }
    /// Atomically transfers private protocol pipes; the original child remains sole waiter.
    pub fn take_private_storage_io(
        &mut self,
        expected: &ProcessIdentity,
    ) -> io::Result<crate::process::PrivateStorageIo> {
        if self.initialization != SpawnInitialization::Ready
            || self.registered.as_ref() != Some(expected)
            || self.reaped
            || self.wait_authority_lost
            || self.authority_unknown
            || self.capture != Capture::default()
            || self.capture_transferred
            || self.stdin.is_none()
            || self.stdout.is_none()
            || self.stderr.is_none()
        {
            return Err(io::Error::other("storage handoff refused before transfer"));
        }
        let (Some(input), Some(output), Some(error)) =
            (self.stdin.take(), self.stdout.take(), self.stderr.take())
        else {
            return Err(io::Error::other("storage handoff invariant violated"));
        };
        self.capture_transferred = true;
        Ok(crate::process::PrivateStorageIo::new(input, output, error))
    }
    /// Transfer original protocol endpoints; this handle keeps exclusive wait custody.
    pub fn take_owner_channel(
        &mut self,
        expected: &ProcessIdentity,
        cap: usize,
    ) -> io::Result<(crate::task_runtime::channel::Channel, File)> {
        if !(5..=65536).contains(&cap) {
            return Err(io::Error::other("owner channel cap refused"));
        }
        let io = self.take_private_storage_io(expected)?;
        let (input, output, error) = io.into_unused_parts()?;
        crate::task_runtime::channel::Channel::new(output, input, cap)
            .map(|channel| (channel, error))
    }
}
