use super::*;
use hf_domain::process::ProcessExit;
impl StorageCoordinator {
    pub(super) fn project(
        &self,
        command: &StorageCommand,
    ) -> Result<(Option<Snapshot>, Option<CaptureSnapshot>), CoordinatorError> {
        if self.closed || self.state.request.payload.len() > 16384 {
            return Err(CoordinatorError::InvalidState);
        }
        let mut s = self
            .state
            .snapshot
            .clone()
            .unwrap_or_else(|| self.state.prepared());
        let mut c = self.state.capture.clone();
        match command {
            StorageCommand::Bootstrap(b) if !self.bootstrapped => {
                if b.request != self.state.request
                    || b.participant.owner != self.state.owner
                    || b.participant.operation != b.request.operation
                    || b.participant.attempt != b.request.attempt
                    || b.participant.request_digest != b.request.digest
                    || b.participant.source.generation != b.request.generation
                {
                    return Err(CoordinatorError::InvalidState);
                }
            }
            StorageCommand::Claim | StorageCommand::RecordChild(_) | StorageCommand::Accept
                if self.bootstrapped
                    && (!matches!(command, StorageCommand::Accept) || self.raw_files.is_some()) =>
            {
                s = self
                    .state
                    .next_snapshot(command)
                    .ok_or(CoordinatorError::InvalidState)?;
            }
            StorageCommand::CreateRaw
                if s.phase == Phase::SpawnObserved && self.raw_files.is_none() => {}
            StorageCommand::CaptureBegin(b) => {
                let raw = self
                    .raw_files
                    .as_ref()
                    .ok_or(CoordinatorError::InvalidState)?;
                if b.stdout != raw.stdout || b.stderr != raw.stderr {
                    return Err(CoordinatorError::InvalidState);
                }
                c = Some(
                    self.state
                        .capture_start(b)
                        .ok_or(CoordinatorError::InvalidState)?,
                );
            }
            StorageCommand::Append {
                stream,
                bytes,
                drain,
            } => {
                c = Some(self.append_projection(*stream, bytes, drain.as_ref())?);
            }

            StorageCommand::KnownLoss => {
                let mut next = self
                    .state
                    .capture
                    .as_ref()
                    .ok_or(CoordinatorError::InvalidState)?
                    .progress
                    .clone();
                next.loss = true;
                next.revision += 1;
                c = Some(
                    self.state
                        .capture_next(&next)
                        .ok_or(CoordinatorError::InvalidState)?,
                );
            }

            StorageCommand::Finish(exit) => {
                c = Some(
                    self.state
                        .capture_terminal(*exit)
                        .ok_or(CoordinatorError::InvalidState)?,
                );
            }
            StorageCommand::Close if self.bootstrapped => {}
            _ => return Err(CoordinatorError::InvalidState),
        }
        Ok((Some(s), c))
    }
    fn append_projection(
        &self,
        stream: Stream,
        bytes: &[u8],
        drain: Option<&TrustedDrainEvidence>,
    ) -> Result<CaptureSnapshot, CoordinatorError> {
        let current = self
            .state
            .capture
            .as_ref()
            .ok_or(CoordinatorError::InvalidState)?;
        let mut next = current.progress.clone();
        if bytes.len() > 16384 || current.exit != ProcessExit::Unobserved {
            return Err(CoordinatorError::InvalidState);
        }
        match stream {
            Stream::Stdout => next.stdout += bytes.len() as u64,
            Stream::Stderr => next.stderr += bytes.len() as u64,
        }
        if let Some(d) = drain {
            if !bytes.is_empty()
                || !d.reader_joined
                || !d.accepted_data_reconciled
                || !d.input_reconciled
            {
                return Err(CoordinatorError::InvalidState);
            }
            next.stdout_eof = d.stdout_eof;
            next.stderr_eof = d.stderr_eof;
        }
        next.revision += 1;
        self.state
            .capture_next(&next)
            .ok_or(CoordinatorError::InvalidState)
    }
}
