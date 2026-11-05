//! Private isolated mechanics owner. No production permission or caller admission.
use super::{
    actor::{Ack, Actor},
    capture::Capture,
    retention::Resources,
};
use hf_application::{owner_runtime::StorageCoordinator, process::IdentityPort};
use hf_domain::{
    fixture::*, owner_policy::FixtureAssignment, owner_runtime::*, ownership::*, process::*,
};
use hf_interfaces::{
    fixture as wire, owner_preparation as prep, owner_storage_codec::participant_digest,
};
use hf_platform::{
    fixture::{PrivateIo, check_entry, qualify_ledger},
    process::*,
};
use hf_storage::{RootHold, Sha256Digest};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[path = "owner/setup.rs"]
mod setup;
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Progress {
    pub(crate) ticks: u64,
    pub(crate) writer_pending: bool,
    pub(crate) agent_exit_observed: bool,
    pub(crate) reader_joined: bool,
    pub(crate) reader_fifo_reconciled: bool,
}
pub(super) struct OwnerLoop {
    pub(super) resources: Resources,
    pub(super) actor: Actor,
    prepared: Prepared,
    cwd: PathBuf,
    executable: PathBuf,
    capture: Capture,
    input: Option<(Vec<u8>, usize)>,
    input_done: bool,
    accepted: bool,
    capture_started: bool,
    closed: bool,
    until: Instant,
    progress: Progress,
}
pub(super) fn run() -> Result<(), String> {
    check_entry().map_err(error)?;
    let until = Instant::now() + Duration::from_secs(8);
    let channel = PrivateIo::stdio(until);
    let prepared = setup::handshake(&channel)?;
    let mut owner = start(prepared, until)?;
    let result = owner.drive(|_| {});
    if result.is_err() {
        owner.poison();
    }
    if !owner.resources.settled(Some(&owner.actor)) {
        owner.resources.retain(Some(&mut owner.actor));
    }
    result?;
    let snapshot = owner
        .actor
        .coordinator
        .snapshot()
        .ok_or("owner snapshot absent")?;
    let frame = Frame::Accepted {
        invitation: owner.prepared.invitation.clone(),
        fence: snapshot.fence().ok_or("owner fence absent")?,
        child: snapshot.child.clone().ok_or("owner child absent")?,
    };
    channel
        .write_packet(&wire::encode(&frame).map_err(error)?)
        .map_err(error)
}
impl OwnerLoop {
    /// Test observers run on the actual owner thread after a complete bounded step.
    pub(super) fn drive(&mut self, mut observe: impl FnMut(&Progress)) -> Result<(), String> {
        loop {
            if Instant::now() >= self.until {
                return Err("owner qualification expired".into());
            }
            if let Some(ack) = self.actor.step()? {
                self.ack(ack)?;
            }
            let (_, exit) = self.resources.exits();
            self.input_step()?;
            if let (Some(reader), Some(agent)) = (&mut self.resources.reader, &self.resources.agent)
            {
                let child = agent
                    .registered_identity()
                    .ok_or("agent identity unknown")?;
                self.capture
                    .poll(reader, &self.prepared.request.operation, child)?;
                self.resources.reader_joined = self.capture.final_value.is_some();
            }
            self.advance(exit)?;
            self.resources.reader_fifo_reconciled = self.capture.reconciled();
            self.progress.ticks += 1;
            self.progress.writer_pending = !self.actor.idle();
            self.progress.agent_exit_observed = exit != ProcessExit::Unobserved;
            self.progress.reader_joined = self.resources.reader_joined;
            self.progress.reader_fifo_reconciled = self.capture.reconciled();
            observe(&self.progress);
            if self.closed && self.resources.settled(Some(&self.actor)) {
                return Ok(());
            }
        }
    }
    fn poison(&mut self) {
        self.actor.poison();
        let pending_append = self
            .actor
            .coordinator
            .pending()
            .is_some_and(|p| matches!(p.command, StorageCommand::Append { .. }));
        let (uncertain, abandoned) = self.capture.abandon_unknown(pending_append);
        self.resources.consumer_uncertain = uncertain;
        self.resources.consumer_abandoned = abandoned;
    }
    fn ack(&mut self, ack: Ack) -> Result<(), String> {
        self.capture.ack(ack)?;
        match ack {
            Ack::Bootstrap => self.actor.submit(StorageCommand::Claim)?,
            Ack::Claim => self.spawn_agent()?,
            Ack::RecordChild => self.actor.submit(StorageCommand::CreateRaw)?,
            Ack::CreateRaw => self.start_reader()?,
            Ack::Accept => {
                self.accepted = true;
                self.begin_capture()?;
            }
            Ack::CaptureBegin => self.capture_started = true,
            Ack::Finish => self.actor.submit(StorageCommand::Close)?,
            Ack::Close => self.closed = true,
            _ => (),
        }
        Ok(())
    }
    fn advance(&mut self, exit: ProcessExit) -> Result<(), String> {
        if !self.actor.idle() {
            return Ok(());
        }
        if self.capture.effect_done && self.input_done && !self.accepted {
            self.actor.submit(StorageCommand::Accept)?;
        } else if self.capture_started && !self.closed {
            self.capture
                .advance(&mut self.actor, exit, self.input_done)?;
        }
        Ok(())
    }
    fn spawn_agent(&mut self) -> Result<(), String> {
        let envelope = prep::decode(&self.prepared.request, &Sha256Digest).map_err(error)?;
        let mode = match envelope.assignment {
            FixtureAssignment::FixedEffect => "--internal-effect-fixture-v1",
            FixtureAssignment::SaturatedEffect => "--internal-effect-saturation-fixture-v1",
        };
        self.resources.agent = Some(setup::spawn_at(
            mode,
            &self.cwd,
            &self.executable,
            self.until,
        )?);
        let child = setup::ready(self.resources.agent.as_ref().ok_or("agent missing")?)?;
        self.actor.submit(StorageCommand::RecordChild(child))
    }
    fn start_reader(&mut self) -> Result<(), String> {
        let snapshot = self
            .actor
            .coordinator
            .snapshot()
            .ok_or("spawn snapshot absent")?;
        let origin = CaptureOrigin::from_spawn_observed(snapshot).map_err(error)?;
        let start = Frame::Start(EffectStart {
            operation: self.prepared.request.operation.clone(),
            root: self.prepared.root.clone(),
            ledger: self.prepared.ledger,
            fence: origin.fence.clone(),
        });
        let bytes = wire::encode(&start).map_err(error)?;
        let agent = self.resources.agent.as_mut().ok_or("agent missing")?;
        self.resources.reader = Some(
            agent
                .start_private_fixture_capture_reader(origin, self.until)
                .map_err(error)?,
        );
        self.input = Some((bytes, 0));
        Ok(())
    }
    fn input_step(&mut self) -> Result<(), String> {
        let Some((bytes, offset)) = &mut self.input else {
            return Ok(());
        };
        let agent = self.resources.agent.as_mut().ok_or("agent missing")?;
        *offset += agent
            .write_stdin(&bytes[*offset..bytes.len().min(*offset + 4096)], self.until)
            .map_err(error)?;
        if *offset == bytes.len() {
            agent.close_stdin();
            self.input = None;
            self.input_done = true;
        }
        Ok(())
    }
    fn begin_capture(&mut self) -> Result<(), String> {
        let snapshot = self
            .actor
            .coordinator
            .snapshot()
            .ok_or("accept snapshot absent")?;
        let raw = self
            .actor
            .coordinator
            .raw_files()
            .ok_or("raw identities absent")?;
        let binding = hf_domain::capture::CaptureBinding {
            fence: snapshot.fence().ok_or("accepted fence absent")?,
            child: snapshot.child.clone().ok_or("accepted child absent")?,
            stdout: raw.stdout.clone(),
            stderr: raw.stderr.clone(),
        };
        self.actor
            .submit(StorageCommand::CaptureBegin(Box::new(binding)))
    }
}
fn error(e: impl std::fmt::Debug) -> String {
    format!("isolated owner: {e:?}")
}

/// Shared test seam uses actual children/IPC; no stdio or readiness fabrication.
pub(super) fn start(prepared: Prepared, until: Instant) -> Result<OwnerLoop, String> {
    setup::start(prepared, until)
}
impl Drop for OwnerLoop {
    fn drop(&mut self) {
        if !self.resources.settled(Some(&self.actor)) {
            self.poison();
            self.resources.retain(Some(&mut self.actor));
        }
    }
}

#[cfg(test)]
pub(super) fn start_at(
    prepared: Prepared,
    until: Instant,
    executable: &Path,
) -> Result<OwnerLoop, String> {
    setup::start_at(prepared, until, executable)
}
