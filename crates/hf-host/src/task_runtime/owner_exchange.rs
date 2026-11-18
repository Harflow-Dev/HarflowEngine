//! Owner-side writer protocol: acknowledged command exchange over the writer channel.
use super::{Error, io, owner_runtime::Runtime};
use hf_domain::task_runtime::*;
use hf_interfaces::task_runtime::*;
impl Runtime {
    pub(super) fn writer_channel(
        &mut self,
    ) -> Result<&mut hf_platform::task_runtime::channel::Channel, Error> {
        self.writer
            .as_mut()
            .ok_or(Error::Refused)?
            .protocol_channel()
    }
    pub(super) fn receive_ack(&mut self) -> Result<WriterAcknowledgement, Error> {
        let frame = io::receive(self.writer_channel()?, io::deadline())?;
        let ack = decode_writer_ack(io::payload(&frame)?).map_err(|_| Error::Refused)?;
        let expected = self.envelope.as_ref().ok_or(Error::Refused)?;
        if &ack.envelope != expected {
            return Err(Error::Refused);
        }
        if self
            .source_binding
            .as_ref()
            .is_some_and(|binding| ack.source_binding.as_ref() != Some(binding))
        {
            return Err(Error::Refused);
        }
        if self.source_binding.is_none() {
            self.source_binding = ack.source_binding.clone();
        }
        Ok(ack)
    }
    /// Capture ends the writer must acknowledge after `operation`.
    fn expected_ends(&self, operation: &WriterOperation) -> Result<(u64, u64), Error> {
        let mut expected_out = self.stdout_end;
        let mut expected_err = self.stderr_end;
        if let WriterOperation::Capture {
            stderr,
            offset,
            bytes,
        } = operation
        {
            let expected = if *stderr {
                &mut expected_err
            } else {
                &mut expected_out
            };
            if *offset != *expected {
                return Err(Error::Refused);
            }
            *expected = expected
                .checked_add(bytes.len() as u64)
                .ok_or(Error::Refused)?;
        }
        Ok((expected_out, expected_err))
    }
    pub(super) fn exchange(
        &mut self,
        operation: WriterOperation,
    ) -> Result<WriterAcknowledgement, Error> {
        let current = self.envelope.as_ref().ok_or(Error::Refused)?.clone();
        let (expected_out, expected_err) = self.expected_ends(&operation)?;
        let terminal = matches!(operation, WriterOperation::Finish { .. });
        let ownership_increment = u64::from(matches!(operation, WriterOperation::Spawn { .. }));
        let command = WriterCommand {
            envelope: current.clone(),
            operation,
        };
        let bytes = encode_writer_command(&command).map_err(|_| Error::Refused)?;
        io::send(self.writer_channel()?, io::frame(bytes), io::deadline())?;
        let mut next = current;
        next.revision += 1;
        next.ownership_revision += ownership_increment;
        self.envelope = Some(next);
        let ack = self.receive_ack()?;
        if ack.stdout_end != expected_out
            || ack.stderr_end != expected_err
            || ack.terminal != terminal
            || ack.source_binding.is_none()
        {
            return Err(Error::Refused);
        }
        self.stdout_end = ack.stdout_end;
        self.stderr_end = ack.stderr_end;
        if ack.close_input && !self.input_closed {
            self.agent.as_mut().ok_or(Error::Refused)?.close_stdin();
            self.input_closed = true;
        }
        Ok(ack)
    }
}
