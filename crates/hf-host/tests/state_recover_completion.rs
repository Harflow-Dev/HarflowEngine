#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Internal B5 staged completion with actual sessions, filesystem faults and restart.
//! No public ctl entry or real engine state is used.
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

#[path = "state_recover_flow/completion.rs"]
mod completion;
#[path = "state_recover_flow/completion_faults.rs"]
mod completion_faults;
#[path = "state_recover_flow/completion_restart.rs"]
mod completion_restart;
#[path = "state_recover_flow/completion_support.rs"]
mod completion_support;
#[path = "state_recover_flow/fault_trace.rs"]
mod faults;
#[path = "state_recover_flow/support.rs"]
mod support;
