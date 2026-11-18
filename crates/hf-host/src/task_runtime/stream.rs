//! Backend observations consume only positively committed original raw bytes.
//!
//! A line is validated completely before it changes the observation. Malformed output
//! never poisons the writer: the stream stops decoding, records a gap and keeps accepting
//! raw bytes; a result accepted on an earlier line stands.
use super::Error;
use hf_domain::{
    task_lifecycle::{ExecutionObservation, ResultWitness},
    task_runtime::ProductionInvitation,
};
use hf_interfaces::claude_ndjson::{Decoder, Limits, Outcome};
/// Why a line was not accepted.
pub(super) enum Fault {
    /// The agent's output broke the stream contract.
    Malformed,
    /// A local failure unrelated to the agent's output; it still poisons the writer.
    Internal(Error),
}
impl From<Error> for Fault {
    fn from(error: Error) -> Self {
        Self::Internal(error)
    }
}
/// Result facts of the current line, committed only after the whole line is valid.
#[derive(Default)]
pub(super) struct Staged {
    pub(super) result: Option<ResultWitness>,
    pub(super) summary: Option<(String, ResultWitness)>,
    pub(super) terminal: bool,
}
pub struct Stream {
    decoder: Decoder,
    pub(super) session: String,
    pub(super) attempt: hf_domain::ownership::Id,
    pub(super) initialized: bool,
    pub(super) terminal: bool,
    pub(super) tools: Vec<(Option<String>, String)>,
    pub(super) seen: Vec<(Option<String>, String)>,
    pub(super) offset: u64,
    pub(super) line: Vec<u8>,
    pub(super) task: hf_domain::task_read::TaskId,
    pub(super) next_seq: u64,
    pub(super) turn_open: bool,
    pub(super) blocks: Vec<u64>,
    summary: Option<(String, ResultWitness)>,
    malformed: bool,
    pub(super) staged: Staged,
}
impl Stream {
    pub fn new(v: &ProductionInvitation) -> Result<Self, Error> {
        Ok(Self {
            decoder: Decoder::new_production(v.binding.attempt.clone(), Limits::default())
                .map_err(|_| Error::Refused)?,
            session: v.session.as_str().into(),
            attempt: v.binding.attempt.clone(),
            initialized: false,
            terminal: false,
            tools: Vec::new(),
            seen: Vec::new(),
            offset: 0,
            line: Vec::new(),
            task: v.binding.task.clone(),
            next_seq: 1,
            turn_open: false,
            blocks: Vec::new(),
            summary: None,
            malformed: false,
            staged: Staged::default(),
        })
    }
    /// Malformed output returns the events of earlier lines and marks the stream; only a
    /// local failure is an error.
    pub fn consume(
        &mut self,
        bytes: &[u8],
        observation: &mut ExecutionObservation,
    ) -> Result<Vec<hf_storage::EventAppend>, Error> {
        #[cfg(test)]
        super::seams::stream_hook(self);
        let result = self.consume_batch(bytes, observation);
        #[cfg(test)]
        super::seams::stream_hook(self);
        result
    }
    fn consume_batch(
        &mut self,
        bytes: &[u8],
        observation: &mut ExecutionObservation,
    ) -> Result<Vec<hf_storage::EventAppend>, Error> {
        let mut events = Vec::new();
        if self.malformed {
            return Ok(events);
        }
        match self.feed_all(bytes, observation, &mut events) {
            Ok(()) => Ok(events),
            Err(Fault::Malformed) => {
                self.malformed = true;
                self.staged = Staged::default();
                self.line.clear();
                observation.observation_gap = true;
                Ok(events)
            }
            Err(Fault::Internal(error)) => Err(error),
        }
    }
    fn feed_all(
        &mut self,
        bytes: &[u8],
        observation: &mut ExecutionObservation,
        events: &mut Vec<hf_storage::EventAppend>,
    ) -> Result<(), Fault> {
        let mut retention_full = false;
        let mut at = 0;
        while at < bytes.len() {
            let feed = self.decoder.feed(&bytes[at..]);
            if feed.consumed == 0 {
                return Err(Fault::Malformed);
            }
            self.line.extend_from_slice(&bytes[at..at + feed.consumed]);
            if self.line.len() > 1_048_577 {
                return Err(Fault::Malformed);
            }
            at += feed.consumed;
            self.offset += feed.consumed as u64;
            match feed.outcome {
                Outcome::Event(records) => {
                    self.line_records(records, observation, events, &mut retention_full)?;
                }
                Outcome::NeedMore => (),
                _ => return Err(Fault::Malformed),
            }
        }
        Ok(())
    }
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }
    /// The successful root summary; never present once the stream is malformed.
    pub fn summary(&self) -> Option<(&str, &ResultWitness)> {
        if self.malformed {
            return None;
        }
        self.summary
            .as_ref()
            .map(|(text, result)| (text.as_str(), result))
    }
    /// Publishes the staged facts of a fully valid line.
    pub(super) fn commit(&mut self, observation: &mut ExecutionObservation) {
        let staged = std::mem::take(&mut self.staged);
        self.terminal |= staged.terminal;
        if staged.result.is_some() {
            observation.result = staged.result;
        }
        if staged.summary.is_some() {
            self.summary = staged.summary;
        }
    }
    /// Closing the agent's input follows a completed turn or any malformed output.
    pub fn close_input(&self) -> bool {
        self.malformed
            || (self.terminal && self.tools.is_empty() && !self.turn_open && self.blocks.is_empty())
    }
    pub fn malformed(&self) -> bool {
        self.malformed
    }
    /// A partial last line at exit counts as malformed. Returns whether the stream is
    /// malformed.
    pub fn finish(&mut self) -> bool {
        if !self.malformed && self.decoder.finish().is_err() {
            self.malformed = true;
        }
        self.malformed
    }
}
