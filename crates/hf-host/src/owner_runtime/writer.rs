//! Isolated private storage child. No public runtime, backend or caller admission.
use hf_application::{
    owner::OwnerSession, owner_runtime::StorageCoordinator, process::IdentityPort,
};
use hf_domain::{capture::*, fixture::Stage, owner_runtime::*, ownership::*};
use hf_interfaces::owner_storage_codec::{self as codec, Message};
use hf_platform::{
    fixture::{FixtureGate, PrivateIo, RawCapture, check_entry},
    process::{MacIdentity, current_parent_identity},
};
use hf_storage::{RootHold, Sha256Digest, Store};
use std::{
    path::Path,
    time::{Duration, Instant},
};
mod execute;
mod state;
use state::Writer;

pub(super) fn run(qualification: bool) -> Result<(), String> {
    check_entry().map_err(error)?;
    let until = Instant::now() + Duration::from_secs(if qualification { 100 } else { 8 });
    let channel = PrivateIo::stdio(until);
    let first = receive(&channel)?;
    let Message::Command(StorageCommand::Bootstrap(bootstrap)) = first.message else {
        return Err("storage bootstrap required".into());
    };
    if qualification != bootstrap.request.payload.starts_with(b"HFCQ") {
        return Err("storage qualification mode mismatch".into());
    }
    if first.identity.sequence != 1 {
        return Err("storage bootstrap sequence".into());
    }
    let mut writer = Writer::bootstrap(&bootstrap, until)?;
    let mut coordinator = StorageCoordinator::new(
        bootstrap.request.clone(),
        bootstrap.participant.owner.clone(),
        first.identity.binding_digest.clone(),
    );
    let bootstrap_command = StorageCommand::Bootstrap(bootstrap);
    coordinator
        .begin(
            bootstrap_command,
            first.identity.command_digest.clone(),
            until,
            Instant::now(),
        )
        .map_err(error)?;
    coordinator
        .send_complete(&first.identity, Instant::now())
        .map_err(error)?;
    let ack = writer.ack(first.identity, AckDisposition::Committed);
    coordinator
        .acknowledge(ack.clone(), Instant::now())
        .map_err(error)?;
    send(&channel, &ack)?;
    loop {
        let frame = receive(&channel)?;
        let Message::Command(command) = frame.message else {
            return Err("storage command required".into());
        };
        if frame.identity.sequence != coordinator.next_sequence()
            || &frame.identity.binding_digest != coordinator.binding_digest()
        {
            return Err("storage command identity".into());
        }
        coordinator
            .begin(
                command,
                frame.identity.command_digest.clone(),
                until,
                Instant::now(),
            )
            .map_err(error)?;
        coordinator
            .send_complete(&frame.identity, Instant::now())
            .map_err(error)?;
        let pending = coordinator
            .pending()
            .ok_or("missing pending storage command")?;
        let close = matches!(pending.command, StorageCommand::Close);
        if writer.execute(&pending.command).is_err() {
            let ack = writer.ack(frame.identity, AckDisposition::Unknown);
            let _ = send(&channel, &ack);
            return Err("storage mutation uncertain".into());
        }
        let ack = writer.ack(frame.identity, AckDisposition::Committed);
        coordinator
            .acknowledge(ack.clone(), Instant::now())
            .map_err(error)?;
        send(&channel, &ack)?;
        if close {
            channel.expect_eof().map_err(error)?;
            return Ok(());
        }
    }
}
fn receive(channel: &PrivateIo) -> Result<codec::Frame, String> {
    let bytes = channel.read_packet().map_err(error)?;
    codec::decode(&bytes, &Sha256Digest).map_err(error)
}
fn send(channel: &PrivateIo, ack: &StorageAck) -> Result<(), String> {
    let bytes = codec::encode(
        &Message::Ack(Box::new(ack.clone())),
        ack.identity.sequence,
        &ack.identity.binding_digest,
        &Sha256Digest,
    )
    .map_err(error)?;
    channel.write_packet(&bytes).map_err(error)
}
fn error<E: std::fmt::Debug>(e: E) -> String {
    format!("private storage: {e:?}")
}
