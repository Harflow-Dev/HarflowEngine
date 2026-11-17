//! Storage-free original-owner observation and uncertainty retention.
use super::{Error, io, owner_runtime::Runtime};
use hf_domain::{process::ProcessExit, task_runtime::WriterOperation};
use hf_platform::{process::CaptureReader, task_runtime::settlement::OwnerSettlement};
use std::{sync::mpsc::TryRecvError, time::Duration};
impl Runtime {
    pub(super) fn tick(&mut self) -> Result<bool, Error> {
        let reader = self.reader.as_ref().ok_or(Error::Refused)?;
        let progress = reader.view().snapshot();
        if progress.discard || progress.stdout.error.is_some() || progress.stderr.error.is_some() {
            return Err(Error::Poisoned);
        }
        match reader.try_recv() {
            Ok(batch) => {
                if Some(&batch.origin) != self.origin.as_ref() {
                    return Err(Error::Refused);
                }
                if !batch.stdout.is_empty() {
                    self.exchange(WriterOperation::Capture {
                        stderr: false,
                        offset: self.stdout_end,
                        bytes: batch.stdout,
                    })?;
                }
                if !batch.stderr.is_empty() {
                    self.exchange(WriterOperation::Capture {
                        stderr: true,
                        offset: self.stderr_end,
                        bytes: batch.stderr,
                    })?;
                }
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => (),
        }
        let exit = self.agent.as_mut().ok_or(Error::Refused)?.try_wait()?;
        if exit != ProcessExit::Unobserved && !self.input_closed {
            self.agent.as_mut().ok_or(Error::Refused)?.close_stdin();
            self.input_closed = true;
        }
        if exit != ProcessExit::Unobserved
            && self.reader.as_ref().is_some_and(CaptureReader::is_finished)
        {
            let final_value = self
                .reader
                .as_mut()
                .ok_or(Error::Refused)?
                .join_finished()?;
            // Empty FIFO is checked without consuming an uncommitted batch by the proof.
            if self.stdout_end == final_value.progress.stdout.enqueued
                && self.stderr_end == final_value.progress.stderr.enqueued
            {
                let proof = OwnerSettlement::qualify(
                    self.agent.as_ref().ok_or(Error::Refused)?,
                    self.reader.as_ref().ok_or(Error::Refused)?,
                    self.origin.as_ref().ok_or(Error::Refused)?,
                    (self.stdout_end, self.stderr_end, self.input_bytes),
                    self.source_binding.clone().ok_or(Error::Refused)?,
                )?;
                let ack = self.exchange(proof.finish_operation(super::timestamp()?))?;
                if !ack.terminal {
                    return Err(Error::Refused);
                }
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(super) fn retain_uncertain(&mut self) -> ! {
        if let Some(reader) = &self.reader {
            reader.discard_capture();
        }
        loop {
            if let Some(reader) = &self.reader {
                while reader.try_recv().is_ok() {}
                if reader.is_finished() {
                    let _ = self.reader.as_mut().map(CaptureReader::join_finished);
                }
            }
            if let Some(agent) = &mut self.agent {
                if self.reader.is_none() {
                    let _ = agent.pump(io::deadline(), 32768);
                }
                let _ = agent.try_wait();
            }
            if let Some(discovery) = &mut self.discovery
                && let Some(probe) = discovery.pending_probe()
            {
                let _ = probe.pump(io::deadline(), 32768);
                let _ = probe.try_wait();
            }
            if let Some(writer) = &mut self.writer {
                writer.drain_step();
                let _ = writer.child().try_wait();
            }
            self.entry.channel().discard_step();
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    pub(super) fn settle_writer(&mut self) -> Result<(), Error> {
        let writer = self.writer.as_mut().ok_or(Error::Refused)?;
        loop {
            let exit = writer.child().try_wait()?;
            let drained = writer.drain_step();
            let out = matches!(
                drained.stdout.as_ref().map(|s| &s.read),
                Some(hf_platform::task_runtime::channel::DiscardRead::Eof)
            );
            let err = matches!(drained.stderr, super::StderrDrain::Eof);
            if exit == ProcessExit::Code(0) && out && err {
                return Ok(());
            }
            if exit != ProcessExit::Unobserved && exit != ProcessExit::Code(0) {
                return Err(Error::Refused);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
