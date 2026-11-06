//! One retained reader batch; each stream waits for its exact storage acknowledgment.
use super::actor::{Ack, Actor};
use hf_domain::{
    fixture::Frame,
    owner_runtime::*,
    process::{ProcessExit, ProcessIdentity},
};
use hf_interfaces::fixture;
use hf_platform::process::{CaptureBatch, PrivateFixtureCaptureReader, ReaderFinal};

#[derive(Default)]
pub(super) struct Capture {
    hashes: Option<(hf_storage::Sha256Stream, hf_storage::Sha256Stream)>,
    batch: Option<CaptureBatch>,
    awaiting: Option<(Stream, usize)>,
    protocol: Vec<u8>,
    pub(super) effect_done: bool,
    seen: (u64, u64),
    acknowledged: (u64, u64),
    abandoned: (u64, u64),
    uncertain: (u64, u64),
    pub(super) final_value: Option<ReaderFinal>,
    fifo_empty: bool,
    pub(super) eof_sent: bool,
    pub(super) loss_sent: bool,
}
impl Capture {
    pub(super) fn enable_commitment(&mut self) -> Result<(), String> {
        self.hashes = Some((
            hf_storage::Sha256Stream::with_limit(65536).map_err(error)?,
            hf_storage::Sha256Stream::with_limit(65536).map_err(error)?,
        ));
        Ok(())
    }
    pub(super) fn commitment(
        &mut self,
    ) -> Result<(hf_domain::ownership::Digest, hf_domain::ownership::Digest), String> {
        if !self.reconciled()
            || self.loss_sent
            || self.abandoned != (0, 0)
            || self.uncertain != (0, 0)
            || !self.eof_sent
        {
            return Err("capture commitment incomplete".into());
        }
        let (stdout, stderr) = self.hashes.take().ok_or("capture commitment absent")?;
        Ok((
            stdout.finish().map_err(error)?,
            stderr.finish().map_err(error)?,
        ))
    }
    pub(super) fn abandon_unknown(&mut self, pending_append: bool) -> ((u64, u64), (u64, u64)) {
        if let Some((stream, count)) = self.awaiting.take() {
            let target = if pending_append {
                &mut self.uncertain
            } else {
                &mut self.abandoned
            };
            match stream {
                Stream::Stdout => target.0 += count as u64,
                Stream::Stderr => target.1 += count as u64,
            }
        }
        if let Some(batch) = self.batch.take() {
            self.abandoned.0 += batch.stdout.len() as u64;
            self.abandoned.1 += batch.stderr.len() as u64;
        }
        (self.uncertain, self.abandoned)
    }
    pub(super) fn reconciled(&self) -> bool {
        self.fifo_empty
            && self.batch.is_none()
            && self.awaiting.is_none()
            && self.seen
                == (
                    self.acknowledged.0 + self.protocol.len() as u64 + self.abandoned.0,
                    self.acknowledged.1 + self.abandoned.1,
                )
    }
    pub(super) fn poll(
        &mut self,
        reader: &mut PrivateFixtureCaptureReader,
        operation: &hf_domain::ownership::Id,
        child: &ProcessIdentity,
    ) -> Result<(), String> {
        if reader.is_finished() && self.final_value.is_none() {
            self.final_value = Some(reader.join_finished().map_err(error)?);
        }
        if self.batch.is_some() {
            return Ok(());
        }
        match reader.try_recv() {
            Ok(mut batch) => {
                if &batch.origin != reader.origin() {
                    return Err("reader origin mismatch".into());
                }
                self.fifo_empty = false;
                self.seen.0 += batch.stdout.len() as u64;
                self.seen.1 += batch.stderr.len() as u64;
                if !self.effect_done {
                    self.protocol(&mut batch.stdout, operation, child)?;
                }
                trim(
                    &mut batch.stdout,
                    65536usize.saturating_sub(self.acknowledged.0 as usize),
                    &mut self.abandoned.0,
                );
                trim(
                    &mut batch.stderr,
                    65536usize.saturating_sub(self.acknowledged.1 as usize),
                    &mut self.abandoned.1,
                );
                if !batch.stdout.is_empty() || !batch.stderr.is_empty() {
                    self.batch = Some(batch);
                }
            }
            Err(
                std::sync::mpsc::TryRecvError::Empty | std::sync::mpsc::TryRecvError::Disconnected,
            ) => {
                self.fifo_empty = self.final_value.is_some();
            }
        }
        Ok(())
    }
    fn protocol(
        &mut self,
        bytes: &mut Vec<u8>,
        operation: &hf_domain::ownership::Id,
        child: &ProcessIdentity,
    ) -> Result<(), String> {
        let mut used = 0;
        while used < bytes.len() && !self.effect_done {
            if self.protocol.len() >= 512 {
                return Err("effect protocol bound".into());
            }
            self.protocol.push(bytes[used]);
            used += 1;
            if self.protocol.len() < 4 {
                continue;
            }
            let size = u32::from_be_bytes([
                self.protocol[0],
                self.protocol[1],
                self.protocol[2],
                self.protocol[3],
            ]) as usize
                + 4;
            if !(10..=512).contains(&size) {
                return Err("effect protocol length".into());
            }
            if self.protocol.len() == size {
                if fixture::decode(&self.protocol).map_err(error)?
                    != (Frame::EffectDone {
                        operation: operation.clone(),
                        process: child.clone(),
                    })
                {
                    return Err("effect protocol identity".into());
                }
                self.effect_done = true;
            }
        }
        bytes.drain(..used);
        Ok(())
    }
    pub(super) fn ack(&mut self, ack: Ack) -> Result<(), String> {
        if ack != Ack::Append {
            return Ok(());
        }
        if let Some((stream, count)) = self.awaiting.take() {
            match stream {
                Stream::Stdout => self.acknowledged.0 += count as u64,
                Stream::Stderr => self.acknowledged.1 += count as u64,
            }
        }
        if self
            .batch
            .as_ref()
            .is_some_and(|b| b.stdout.is_empty() && b.stderr.is_empty())
        {
            self.batch = None;
        }
        Ok(())
    }
    pub(super) fn advance(
        &mut self,
        actor: &mut Actor,
        exit: ProcessExit,
        input_done: bool,
    ) -> Result<(), String> {
        if !actor.idle() {
            return Ok(());
        }
        if let Some(batch) = &mut self.batch {
            let (stream, bytes) = if !batch.stdout.is_empty() {
                (Stream::Stdout, std::mem::take(&mut batch.stdout))
            } else {
                (Stream::Stderr, std::mem::take(&mut batch.stderr))
            };
            if let Some((stdout, stderr)) = &mut self.hashes {
                match stream {
                    Stream::Stdout => stdout.update(&bytes),
                    Stream::Stderr => stderr.update(&bytes),
                }
                .map_err(error)?;
            }
            self.awaiting = Some((stream, bytes.len()));
            return actor.submit(StorageCommand::Append {
                stream,
                bytes,
                drain: None,
            });
        }
        let Some(final_value) = &self.final_value else {
            return Ok(());
        };
        if !self.fifo_empty {
            return Ok(());
        }
        let p = &final_value.progress;
        let expected = (
            self.acknowledged.0 + self.protocol.len() as u64 + self.abandoned.0,
            self.acknowledged.1 + self.abandoned.1,
        );
        if expected != self.seen || (p.stdout.enqueued, p.stderr.enqueued) != self.seen {
            return Err("reader accepted prefix unreconciled".into());
        }
        let loss = p.discard
            || p.stdout.discarded > 0
            || p.stderr.discarded > 0
            || self.abandoned != (0, 0);
        if loss && !self.loss_sent {
            self.loss_sent = true;
            return actor.submit(StorageCommand::KnownLoss);
        }
        if loss
            || final_value.stopped
            || final_value.limited
            || !p.stdout.eof
            || !p.stderr.eof
            || p.stdout.error.is_some()
            || p.stderr.error.is_some()
            || (p.stdout.read, p.stderr.read) != self.seen
        {
            return Err("reader final incomplete".into());
        }
        if !self.eof_sent && input_done {
            self.eof_sent = true;
            return actor.submit(StorageCommand::Append {
                stream: Stream::Stdout,
                bytes: Vec::new(),
                drain: Some(TrustedDrainEvidence {
                    reader_joined: true,
                    accepted_data_reconciled: true,
                    input_reconciled: true,
                    stdout_eof: true,
                    stderr_eof: true,
                }),
            });
        }
        if self.eof_sent
            && exit != ProcessExit::Unobserved
            && actor
                .coordinator
                .capture()
                .is_some_and(|c| c.exit == ProcessExit::Unobserved)
        {
            return actor.submit(StorageCommand::Finish(exit));
        }
        Ok(())
    }
}
fn error(e: impl std::fmt::Debug) -> String {
    format!("owner capture: {e:?}")
}

fn trim(bytes: &mut Vec<u8>, remaining: usize, abandoned: &mut u64) {
    let keep = bytes.len().min(remaining);
    *abandoned += (bytes.len() - keep) as u64;
    bytes.truncate(keep);
}
