//! Actual private owner/writer/reader children over disposable roots.
use super::{RestoreDirectory, owner, support};
use hf_domain::{
    fixture::{Gate, Stage},
    owner_runtime::{StorageCommand, StorageRefusal},
};
use hf_platform::process::current_parent_identity;
use std::fs;
#[path = "b8_controlled.rs"]
mod controlled;

pub(in crate::owner_runtime) fn observe_retention(
    resources: &super::super::retention::Resources,
    actor: Option<&super::super::actor::Actor>,
) {
    controlled::observe(resources, actor);
}

#[test]
fn actual_fenced_writer_refusal_is_known_and_settles_original_children() {
    exercise(StorageRefusal::Fenced);
}

#[test]
fn actual_commit_lost_writer_refusal_is_known_and_settles_original_children() {
    exercise(StorageRefusal::CommitLost);
}

fn exercise(cause: StorageRefusal) {
    let _serial = crate::fixture::test_support::case_guard();
    let scratch = support::Scratch::new(current_parent_identity().unwrap());
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(&scratch.path).unwrap();
    let _restore = RestoreDirectory(previous);
    let (listener, socket) = scratch.gate_listener();
    let stage = Stage::CaptureAfterPublish;
    let prepared = scratch.prepared(
        support::self_identity(),
        Gate {
            stage,
            path: socket.to_string_lossy().into_owned(),
        },
    );
    let mut peer = support::GatePeer::start(
        listener,
        stage,
        prepared.request.operation.clone(),
        prepared.invitation.nonce.clone(),
    );
    let mut owner =
        owner::start_at(prepared, support::deadline(8), &support::built_scaffold()).unwrap();
    let mut injected = false;
    let outcome = owner.drive(|_| {
        if peer.arrived() && !injected {
            inject(&scratch, cause);
            injected = true;
            peer.release();
        }
    });
    peer.release();
    assert!(peer.finish().is_ok());
    assert!(injected && outcome.is_err());
    assert_eq!(owner.actor.coordinator.storage_refusal(), Some(cause));
    assert!(!owner.actor.coordinator.is_poisoned());
    assert_eq!(owner.resources.consumer_uncertain, (0, 0));
    assert_ne!(owner.resources.consumer_known_unindexed, (0, 0));
    assert!(owner.resources.reader_joined && owner.resources.reader_fifo_reconciled);
    assert!(owner.resources.settled(Some(&owner.actor)));
    let sequence = owner.actor.coordinator.next_sequence();
    assert!(
        owner
            .actor
            .submit(StorageCommand::Append {
                stream: hf_domain::owner_runtime::Stream::Stdout,
                bytes: vec![1],
                drain: None,
            })
            .is_err()
    );
    assert!(owner.actor.submit(StorageCommand::KnownLoss).is_err());
    assert!(
        owner
            .actor
            .submit(StorageCommand::Finish(
                hf_domain::process::ProcessExit::Code(0)
            ))
            .is_err()
    );
    assert!(owner.actor.submit(StorageCommand::Close).is_err());
    assert_eq!(owner.actor.coordinator.next_sequence(), sequence);
    super::assert_root_hold_lifetime(&scratch, owner);
}

pub(super) fn inject(scratch: &support::Scratch, cause: StorageRefusal) {
    let generation = scratch.request.generation.as_str();
    match cause {
        StorageRefusal::Fenced => fs::write(
            scratch.root.join("store.fence"),
            format!("harflow-fence-1\n{generation}\nfailed\n"),
        )
        .unwrap(),
        StorageRefusal::CommitLost => {
            let mut bytes = hf_application::watermark::initial_file(generation).unwrap();
            let payload =
                hf_application::watermark::encode_payload(generation, u64::MAX / 2).unwrap();
            bytes[..payload.len()].copy_from_slice(&payload);
            fs::write(scratch.root.join("store.watermark"), bytes).unwrap();
        }
    }
}
