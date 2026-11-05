use super::*;
use hf_storage::Limits;
pub(super) struct Writer {
    pub(super) session: Option<OwnerSession<Store>>,
    pub(super) raw: Option<RawCapture>,
    pub(super) snapshot: Snapshot,
    pub(super) capture: Option<CaptureSnapshot>,
    pub(super) raw_files: Option<RawIdentities>,
    pub(super) root: String,
    pub(super) until: Instant,
    gate: FixtureGate,
    gate_seen: u16,
    nonce: Id,
    _hold: RootHold,
}
impl Writer {
    pub(super) fn bootstrap(b: &StorageBootstrap, until: Instant) -> Result<Self, String> {
        let actual = MacIdentity
            .identity(std::process::id())
            .ok_or("storage process identity unknown")?;
        if actual != b.participant.writer
            || current_parent_identity().as_ref() != Some(&b.participant.owner.process)
        {
            return Err("storage original parent/writer mismatch".into());
        }
        hf_interfaces::owner_preparation::validate(&b.request, &b.participant, &Sha256Digest)
            .map_err(error)?;
        let root = Path::new(&b.root);
        let parent = root.parent().ok_or("storage root parent")?;
        if root.file_name().and_then(|s| s.to_str()) != Some("store")
            || std::fs::canonicalize(parent).map_err(error)? != parent
            || std::env::current_dir().map_err(error)? != parent
        {
            return Err("storage inherited physical cwd mismatch".into());
        }
        if b.gate
            .as_ref()
            .is_some_and(|g| Path::new(&g.path).parent() != Some(parent))
        {
            return Err("storage gate parent mismatch".into());
        }
        let hold = RootHold::open(root, until, hf_platform::lease::shared()).map_err(error)?;
        if hold.source_stamp() != &b.participant.source {
            return Err("storage source changed".into());
        }
        let store = Store::open(root, Limits::default(), until, hf_platform::lease::shared())
            .map_err(error)?;
        let snapshot = store
            .ownership_observe(&b.request.operation, until)
            .map_err(error)?
            .ok_or("prepared row absent")?;
        let expected = Snapshot {
            request: b.request.clone(),
            phase: Phase::Prepared,
            revision: 0,
            cancelled: false,
            owner: None,
            child: None,
        };
        if snapshot != expected {
            return Err("storage prepared row mismatch".into());
        }
        store
            .create_storage_participant(&b.participant, until)
            .map_err(error)?;
        RawCapture::prepare_sentinels(root, &b.request.operation, until).map_err(error)?;
        let writer = Self {
            session: Some(OwnerSession::new(
                store,
                b.request.clone(),
                b.participant.owner.clone(),
                0,
            )),
            raw: None,
            snapshot,
            capture: None,
            raw_files: None,
            root: b.root.clone(),
            until,
            gate: FixtureGate::new(b.gate.clone()),
            gate_seen: 0,
            nonce: b.participant.channel_nonce.clone(),
            _hold: hold,
        };
        Ok(writer)
    }
    pub(super) fn gate(&mut self, stage: Stage) -> Result<(), String> {
        let bit = match stage {
            Stage::OwnerAfterClaim => 1,
            Stage::OwnerAfterObserved => 2,
            Stage::OwnerBeforeAck => 4,
            Stage::CaptureBeforeSync => 8,
            Stage::CaptureAfterSync => 16,
            Stage::CaptureAfterPublish => 32,
            Stage::CaptureBeforeTerminal => 64,
            Stage::CaptureAfterTerminal => 128,
            _ => return Err("storage gate stage".into()),
        };
        if self.gate_seen & bit != 0 {
            return Ok(());
        }
        self.gate_seen |= bit;
        self.gate
            .reach(
                stage,
                &self.snapshot.request.operation,
                &self.nonce,
                self.until,
            )
            .map_err(error)
    }
    pub(super) fn ack(&self, identity: CommandIdentity, disposition: AckDisposition) -> StorageAck {
        StorageAck {
            identity,
            disposition,
            revision: if disposition == AckDisposition::Committed {
                Some(
                    self.capture
                        .as_ref()
                        .map_or(self.snapshot.revision, |c| c.progress.revision),
                )
            } else {
                None
            },
            snapshot: Some(self.snapshot.clone()),
            capture: self.capture.clone(),
            raw_files: self.raw_files.clone(),
        }
    }
    pub(super) fn refresh(&mut self) {
        if let Some(session) = &self.session {
            if let Some(snapshot) = session.snapshot() {
                self.snapshot = snapshot.clone();
            }
            self.capture = session.capture().cloned();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // Keep the independent root lease until raw and SQLite close have returned.
        drop(self.raw.take());
        drop(self.session.take());
    }
}
