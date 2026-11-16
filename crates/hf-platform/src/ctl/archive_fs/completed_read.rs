//! Lease-free, bounded receipt lookup and minimal committed-operation reads.
mod python_state;
use super::{descriptor::*, *};
use hf_application::ctl::{
    state_inspect::StateCodecPort,
    state_recover::{
        completion::{CompletionReadPort, CompletionSnapshot, ReceiptReadPort, ReceiptSnapshot},
        flow::RecoveryRequest,
        journal::JOURNAL_BOUND,
    },
};
use hf_domain::ctl::{
    inspect::{MarkerDecode, RootStat, Scalar},
    state_recovery::RECORD_BOUND,
    state_root::StateRoot,
};

pub struct FsRecoveryRead<'a> {
    root: &'a StateRoot,
    codec: &'a dyn StateCodecPort,
}
impl<'a> FsRecoveryRead<'a> {
    pub fn new(root: &'a StateRoot, codec: &'a dyn StateCodecPort) -> Self {
        Self { root, codec }
    }
    /// The layout-2 refusal precedes confirmation parsing. No current store is read.
    pub fn layout_two_before_confirmation(&self) -> Result<bool, RecoveryError> {
        let Ok(root) = self.root() else {
            return Ok(false);
        };
        self.layout_two(&root)
    }
    fn root(&self) -> Result<OwnedFd, RecoveryError> {
        let path = std::path::Path::new(self.root.as_str());
        let stat = crate::ctl::state_observer::stat_root(path).0;
        if stat != RootStat::Present {
            return Err(root_error(stat));
        }
        // A failed open after safe metadata is not proof of unsafety: retain the
        // current missing/unsafe/unreadable distinction using root metadata only.
        absolute(path).map_err(|_| root_error(crate::ctl::state_observer::stat_root(path).0))
    }

    fn layout_two(&self, root: &OwnedFd) -> Result<bool, RecoveryError> {
        Ok(read(root, ".harflow-store.json", 16_384)?.is_some_and(|bytes| {
            matches!(self.codec.decode_marker(&bytes), MarkerDecode::Store { observed, .. } if observed.iter().any(|field| field.key == "layout_version" && field.value == Scalar::Int(2)))
        }))
    }
}
impl CompletionReadPort for FsRecoveryRead<'_> {
    fn read(&self) -> Result<CompletionSnapshot, RecoveryError> {
        let root = self.root()?;
        let layout_two = self.layout_two(&root)?;
        let root_identity = identity(&root)?;
        if layout_two {
            return Ok(CompletionSnapshot {
                layout_two,
                root_identity,
                record: None,
                journal: None,
            });
        }
        let pending = read(&root, ".harflow-recovery.json.pending", JOURNAL_BOUND)?;
        let journal = match pending {
            Some(bytes) => Some(bytes),
            None => read(&root, ".harflow-recovery.json", JOURNAL_BOUND)?,
        };
        let record = read(&root, "store.recovery", RECORD_BOUND as usize)?;
        if identity(&self.root()?)? != root_identity {
            return Err(RecoveryError::StaleConfirmation);
        }
        Ok(CompletionSnapshot {
            layout_two,
            root_identity,
            record,
            journal,
        })
    }
}
impl ReceiptReadPort for FsRecoveryRead<'_> {
    fn read(&self, request: &RecoveryRequest<'_>) -> Result<ReceiptSnapshot, RecoveryError> {
        if request.root != self.root {
            return Err(RecoveryError::StaleConfirmation);
        }
        let root = self.root()?;
        let layout_two = self.layout_two(&root)?;
        let root_identity = identity(&root)?;
        if layout_two {
            return Ok(ReceiptSnapshot {
                layout_two,
                root_identity,
                backup_identity: None,
                manifest: None,
                receipt: None,
            });
        }
        if !hf_domain::ctl::state_recovery::is_operation_id(request.operation_id)
            || request.proposal.operation_id != request.operation_id
            || request.proposal.engine_version != hf_domain::ctl::identity::PRODUCT_VERSION
            || request.proposal.state_root != self.root.as_str()
            || request.proposal.root_identity != root_identity
        {
            return Err(RecoveryError::StaleConfirmation);
        }
        let (parent, name) = parent(&request.proposal.backup_path())?;
        let mut snapshot = ReceiptSnapshot {
            layout_two,
            root_identity,
            backup_identity: None,
            manifest: None,
            receipt: None,
        };
        if stat(&parent, &name)?.is_some() {
            let backup = directory(&parent, &name)?;
            let backup_identity = identity(&backup)?;
            snapshot.backup_identity = Some(backup_identity);
            snapshot.receipt = read(&backup, "receipt.json", JOURNAL_BOUND)?;
            if snapshot.receipt.is_some() {
                snapshot.manifest = read(&backup, "manifest.json", JOURNAL_BOUND)?;
            }
            let current = directory(&parent, &name)?;
            if identity(&current)? != backup_identity {
                return Err(RecoveryError::BackupConflict);
            }
        }
        if identity(&self.root()?)? != root_identity {
            return Err(RecoveryError::StaleConfirmation);
        }
        Ok(snapshot)
    }
}

fn root_error(stat: RootStat) -> RecoveryError {
    match stat {
        RootStat::Missing => RecoveryError::StaleConfirmation,
        RootStat::Unsafe => RecoveryError::UnsafeRoot,
        RootStat::Unreadable | RootStat::Present => RecoveryError::InfrastructureUnavailable,
    }
}
