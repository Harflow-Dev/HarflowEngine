//! Public operation-bound recovery composition. Original exclusion spans every effect.
use hf_application::ctl::{
    state_inspect::StorePolicy,
    state_recover::{
        completion::{CompletionPorts, completed_result},
        flow::{self, RecoveryError, RecoveryFailure, RecoveryRequest},
        journal::RecoveryResult,
    },
};
use hf_domain::ctl::{identity::PRODUCT_VERSION, state_root::StateRoot};
use hf_interfaces::ctl::{
    recovery_codec::{JsonRecoveryCodec, decode_token},
    recovery_journal::JsonJournalCodec,
    state_codec::JsonStateCodec,
};
use hf_platform::ctl::{
    archive_fs::{FsArchive, FsRecovery, FsRecoveryRead, MacExclusiveRename, NoArchiveFault},
    inventory::FsInventory,
    recovery_fs::MacRecoveryFs,
    state_observer::FsStateObserver,
};
use std::sync::Arc;

pub fn recover(
    root: &StateRoot,
    operation_id: &str,
    token: &str,
) -> Result<RecoveryResult, RecoveryFailure> {
    let read = FsRecoveryRead::new(root, &JsonStateCodec);
    if read.layout_two_before_confirmation()? {
        return Err(RecoveryError::UnsupportedLayout.into());
    }
    let proposal = decode_token(token).map_err(|_| RecoveryError::StaleConfirmation)?;
    if proposal.operation_id != operation_id
        || proposal.state_root != root.as_str()
        || proposal.engine_version != PRODUCT_VERSION
    {
        return Err(RecoveryError::StaleConfirmation.into());
    }
    let request = RecoveryRequest {
        root,
        proposal: &proposal,
        operation_id,
        created_at: "",
    };
    read.refuse_python_state(&proposal)?;
    if let Some(result) = completed_result(&request, &read, &JsonJournalCodec)? {
        return Ok(result);
    }
    read.refuse_python_journal(&proposal)?;
    let created_at = timestamp()?;
    let request = RecoveryRequest {
        created_at: &created_at,
        ..request
    };
    let session = hf_storage::MaintenanceSession::acquire_proposal(
        &proposal,
        Arc::new(MacRecoveryFs),
        hf_platform::lease::shared(),
    )?;
    let archive = FsArchive::new(
        &session,
        &MacExclusiveRename,
        &NoArchiveFault,
        &JsonJournalCodec,
    );
    let initializer = session.initializer(&hf_platform::durability::FullFlush);
    let staging = FsRecovery::new(
        &archive,
        &initializer,
        &hf_storage::Sha256Digest,
        &hf_platform::durability::FullFlush,
    );
    flow::recover_qualified(
        &request,
        &flow::RecoveryPorts {
            session: &session,
            evidence: &session,
            observer: &FsStateObserver,
            state_codec: &JsonStateCodec,
            policy: StorePolicy {
                schema_version: hf_storage::SCHEMA_VERSION,
                application_id: hf_storage::APPLICATION_ID,
            },
            inventory: &FsInventory,
            inventory_codec: &JsonRecoveryCodec,
            digest: &hf_storage::Sha256Digest,
            identities: &hf_platform::process::MacIdentity,
            journal_codec: &JsonJournalCodec,
            mutations: &archive,
        },
        &CompletionPorts {
            read: &read,
            mutations: &archive,
            staging: &staging,
        },
    )
}
fn timestamp() -> Result<String, RecoveryError> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| RecoveryError::InfrastructureUnavailable)?;
    let seconds =
        i64::try_from(elapsed.as_secs()).map_err(|_| RecoveryError::InfrastructureUnavailable)?;
    let (year, month, day) = hf_domain::timestamp::civil_from_days(seconds.div_euclid(86_400));
    if !(1..=9999).contains(&year) {
        return Err(RecoveryError::InfrastructureUnavailable);
    }
    let time = seconds.rem_euclid(86_400);
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        time / 60 % 60,
        time % 60
    ))
}
