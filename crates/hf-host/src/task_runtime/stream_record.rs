//! Line validation of the backend stream: boundaries, records and event retention.
use super::stream::{Fault, Stream};
use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    backend::{BackendRecord, RecordKind},
    task_lifecycle::{ExecutionObservation, ResultWitness},
};
use hf_interfaces::claude_ndjson::{Boundary, line_boundary};
type Events = Vec<hf_storage::EventAppend>;
impl Stream {
    /// Validates every record of one complete line, projects its events and only then
    /// commits the line's result facts.
    pub(super) fn line_records(
        &mut self,
        records: Vec<BackendRecord>,
        observation: &mut ExecutionObservation,
        events: &mut Events,
        retention_full: &mut bool,
    ) -> Result<(), Fault> {
        self.boundary()?;
        for record in records {
            self.record(record)?;
        }
        let at = super::timestamp()?;
        let mut projected_bytes = events.iter().map(|e| e.raw_len + 1).sum::<u64>();
        let start = self.offset - self.line.len() as u64;
        let projected = hf_interfaces::task_runtime::backend_events(
            &self.line,
            &self.task,
            self.next_seq,
            &at,
            start,
        )
        .map_err(|_| Fault::Malformed)?;
        for event in projected {
            self.retain(
                events,
                observation,
                event,
                retention_full,
                &mut projected_bytes,
            )?;
        }
        self.line.clear();
        self.commit(observation);
        Ok(())
    }
    fn retain(
        &mut self,
        events: &mut Events,
        observation: &mut ExecutionObservation,
        event: hf_interfaces::task_runtime::NormalizedEvent,
        retention_full: &mut bool,
        projected_bytes: &mut u64,
    ) -> Result<(), Fault> {
        observation.event_projection_truncated |= event.truncated;
        let bytes = event.body.len() as u64 + 1;
        if *retention_full
            || events.len() >= 64
            || observation
                .normalized_event_bytes
                .saturating_add(*projected_bytes)
                .saturating_add(bytes)
                > 64 * 1024 * 1024 - 131072
        {
            *retention_full = true;
            observation.omitted_events = observation
                .omitted_events
                .checked_add(1)
                .ok_or(super::Error::Refused)?;
            observation.observation_gap = true;
            observation.event_projection_truncated = true;
            return Ok(());
        }
        *projected_bytes += bytes;
        events.push(hf_storage::EventAppend {
            seq: self.next_seq,
            kind: event.kind,
            raw_len: event.body.len() as u64,
            body: event.body,
        });
        self.next_seq += 1;
        Ok(())
    }
    fn boundary(&mut self) -> Result<(), Fault> {
        match line_boundary(&self.line).map_err(|_| Fault::Malformed)? {
            Boundary::Start if !self.turn_open && self.blocks.is_empty() => self.turn_open = true,
            Boundary::Stop if self.turn_open && self.blocks.is_empty() => self.turn_open = false,
            Boundary::BlockStart(index) if self.turn_open && !self.blocks.contains(&index) => {
                self.blocks.push(index)
            }
            Boundary::BlockStop(index) if self.turn_open => {
                let position = self
                    .blocks
                    .iter()
                    .position(|v| *v == index)
                    .ok_or(Fault::Malformed)?;
                self.blocks.remove(position);
            }
            Boundary::Delta(index) if self.turn_open && self.blocks.contains(&index) => (),
            Boundary::MessageDelta if self.turn_open => (),
            Boundary::None => (),
            _ => return Err(Fault::Malformed),
        }
        Ok(())
    }
    fn record(&mut self, r: BackendRecord) -> Result<(), Fault> {
        if r.cli_session != self.session
            || r.attempt != self.attempt
            || self.terminal
            || self.staged.terminal
        {
            return Err(Fault::Malformed);
        }
        if !self.initialized {
            if r.kind != RecordKind::Init || r.parent.is_some() {
                return Err(Fault::Malformed);
            }
            self.initialized = true;
        } else if r.kind == RecordKind::Init {
            return Err(Fault::Malformed);
        }
        match r.kind {
            RecordKind::ToolUse(id) => {
                let key = (r.parent, id);
                if self.seen.len() >= 128 || self.seen.contains(&key) {
                    return Err(Fault::Malformed);
                }
                self.seen.push(key.clone());
                self.tools.push(key);
            }
            RecordKind::ToolResult { id, is_error: _ } => {
                let key = (r.parent, id);
                let pos = self
                    .tools
                    .iter()
                    .position(|x| x == &key)
                    .ok_or(Fault::Malformed)?;
                self.tools.remove(pos);
            }
            RecordKind::Terminal { failed } if r.parent.is_none() => self.stage_result(failed)?,
            _ => (),
        }
        Ok(())
    }
    /// Stages the root result of the current line; it reaches the observation only when the
    /// whole line is valid.
    fn stage_result(&mut self, failed: bool) -> Result<(), Fault> {
        if !self.tools.is_empty() || self.turn_open || !self.blocks.is_empty() {
            return Err(Fault::Malformed);
        }
        let digest = hf_storage::Sha256Digest.digest_parts(&[&self.line]);
        let witness = ResultWitness {
            turn: self.attempt.clone(),
            digest,
            source_end: self.offset,
            success: !failed,
        };
        if !failed
            && let Some(text) = hf_interfaces::task_runtime::successful_result_summary(&self.line)
                .map_err(|_| Fault::Malformed)?
        {
            self.staged.summary = Some((text, witness.clone()));
        }
        self.staged.terminal = true;
        self.staged.result = Some(witness);
        Ok(())
    }
}
