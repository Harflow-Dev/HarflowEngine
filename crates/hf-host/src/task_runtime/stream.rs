//! Backend observations consume only positively committed original raw bytes.
use super::Error;
use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    backend::{BackendRecord, RecordKind},
    task_lifecycle::{ExecutionObservation, ResultWitness},
    task_runtime::ProductionInvitation,
};
use hf_interfaces::claude_ndjson::{Boundary, Decoder, Limits, Outcome, line_boundary};
pub struct Stream {
    decoder: Decoder,
    session: String,
    attempt: hf_domain::ownership::Id,
    initialized: bool,
    terminal: bool,
    tools: Vec<(Option<String>, String)>,
    seen: Vec<(Option<String>, String)>,
    offset: u64,
    line: Vec<u8>,
    task: hf_domain::task_read::TaskId,
    next_seq: u64,
    turn_open: bool,
    blocks: Vec<u64>,
    summary: Option<(String, ResultWitness)>,
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
        })
    }
    pub fn consume(
        &mut self,
        bytes: &[u8],
        observation: &mut ExecutionObservation,
    ) -> Result<Vec<hf_storage::EventAppend>, Error> {
        let mut events = Vec::new();
        let mut retention_full = false;
        let mut at = 0;
        while at < bytes.len() {
            let feed = self.decoder.feed(&bytes[at..]);
            if feed.consumed == 0 {
                return Err(Error::Refused);
            }
            self.line.extend_from_slice(&bytes[at..at + feed.consumed]);
            if self.line.len() > 1_048_577 {
                return Err(Error::Refused);
            }
            at += feed.consumed;
            self.offset += feed.consumed as u64;
            match feed.outcome {
                Outcome::Event(records) => {
                    self.boundary()?;
                    for record in records {
                        self.record(record, observation)?;
                    }
                    let at = super::timestamp()?;
                    let mut projected_bytes = events
                        .iter()
                        .map(|e: &hf_storage::EventAppend| e.raw_len + 1)
                        .sum::<u64>();
                    for event in hf_interfaces::task_runtime::backend_events(
                        &self.line,
                        &self.task,
                        self.next_seq,
                        &at,
                        self.offset - self.line.len() as u64,
                    )
                    .map_err(|_| Error::Refused)?
                    {
                        self.retain(
                            &mut events,
                            observation,
                            event,
                            &mut retention_full,
                            &mut projected_bytes,
                        )?;
                    }
                    self.line.clear();
                }
                Outcome::NeedMore => (),
                _ => return Err(Error::Refused),
            }
        }
        Ok(events)
    }
    fn retain(
        &mut self,
        events: &mut Vec<hf_storage::EventAppend>,
        observation: &mut ExecutionObservation,
        event: hf_interfaces::task_runtime::NormalizedEvent,
        retention_full: &mut bool,
        projected_bytes: &mut u64,
    ) -> Result<(), Error> {
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
                .ok_or(Error::Refused)?;
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
    fn boundary(&mut self) -> Result<(), Error> {
        match line_boundary(&self.line).map_err(|_| Error::Refused)? {
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
                    .ok_or(Error::Refused)?;
                self.blocks.remove(position);
            }
            Boundary::Delta(index) if self.turn_open && self.blocks.contains(&index) => (),
            Boundary::MessageDelta if self.turn_open => (),
            Boundary::None => (),
            _ => return Err(Error::Refused),
        }
        Ok(())
    }
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }
    pub fn summary(&self) -> Option<(&str, &ResultWitness)> {
        self.summary
            .as_ref()
            .map(|(text, result)| (text.as_str(), result))
    }
    fn record(&mut self, r: BackendRecord, o: &mut ExecutionObservation) -> Result<(), Error> {
        if r.cli_session != self.session || r.attempt != self.attempt || self.terminal {
            return Err(Error::Refused);
        }
        if !self.initialized {
            if r.kind != RecordKind::Init || r.parent.is_some() {
                return Err(Error::Refused);
            }
            self.initialized = true;
        } else if r.kind == RecordKind::Init {
            return Err(Error::Refused);
        }
        match r.kind {
            RecordKind::ToolUse(id) => {
                let key = (r.parent, id);
                if self.seen.len() >= 128 || self.seen.contains(&key) {
                    return Err(Error::Refused);
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
                    .ok_or(Error::Refused)?;
                self.tools.remove(pos);
            }
            RecordKind::Terminal { failed } if r.parent.is_none() => {
                if !self.tools.is_empty() || self.turn_open || !self.blocks.is_empty() {
                    return Err(Error::Refused);
                }
                self.terminal = true;
                let digest = hf_storage::Sha256Digest.digest_parts(&[&self.line]);
                o.result = Some(ResultWitness {
                    turn: self.attempt.clone(),
                    digest,
                    source_end: self.offset,
                    success: !failed,
                });
                if !failed
                    && let Some(text) =
                        hf_interfaces::task_runtime::successful_result_summary(&self.line)
                            .map_err(|_| Error::Refused)?
                {
                    self.summary = Some((text, o.result.clone().ok_or(Error::Refused)?));
                }
            }
            _ => (),
        }
        Ok(())
    }
    pub fn close_input(&self) -> bool {
        self.terminal && self.tools.is_empty() && !self.turn_open && self.blocks.is_empty()
    }
    pub fn finish(&mut self) -> Result<(), Error> {
        self.decoder.finish().map_err(|_| Error::Refused)
    }
}
