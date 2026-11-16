#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Production composition of proposal, held session, evidence, observer and archive.
//! This stops at B4's pending BackedUp handoff; it does not call a public ctl entry.
use hf_application::ctl::{
    state_inspect::{OperationIdPort, ProposalPorts, StorePolicy, inspect_state},
    state_recover::{
        flow::{RecoveryPorts, RecoveryRequest, archive_only as recover},
        journal::JournalPhase,
    },
};
use hf_domain::ctl::state_root::StateRoot;
use hf_interfaces::ctl::{
    recovery_codec::JsonRecoveryCodec, recovery_journal::JsonJournalCodec,
    state_codec::JsonStateCodec,
};
use hf_platform::ctl::{
    archive_fs::{FsArchive, MacExclusiveRename, NoArchiveFault},
    inventory::FsInventory,
    recovery_fs::MacRecoveryFs,
    state_observer::FsStateObserver,
};
use hf_storage::{Limits, MaintenanceSession, Mutation, SessionRequest, Sha256Digest, Store};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[path = "state_recover_flow/benchmarks.rs"]
mod benchmarks;
#[path = "state_recover_flow/faults.rs"]
mod faults;
#[path = "state_recover_flow/restart.rs"]
mod restart;
#[path = "state_recover_flow/support.rs"]
mod support;

#[path = "state_recover_flow/conflicts.rs"]
mod conflicts;

#[path = "state_recover_flow/fault_trace.rs"]
mod fault_trace;
