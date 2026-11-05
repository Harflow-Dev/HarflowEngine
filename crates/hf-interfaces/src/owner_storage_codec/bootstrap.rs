use super::*;
use hf_domain::{
    capture::FileIdentity,
    fixture::{Gate, Stage},
    recovery::SourceStamp,
};
impl write::Writer {
    pub(super) fn bootstrap(&mut self, b: &StorageBootstrap) -> Result<(), CodecError> {
        self.path(&b.root)?;
        let p = &b.participant;
        let s = &p.source;
        self.id(&s.generation);
        for f in [&s.root, &s.database, &s.marker, &s.store_lock, &s.io_lock] {
            self.file(f);
        }
        self.id(&p.operation);
        self.id(&p.attempt);
        for d in [
            &p.request_digest,
            &p.configuration_digest,
            &p.profile_digest,
        ] {
            self.digest(d)?;
        }
        self.owner(&p.owner);
        self.process(&p.writer);
        self.id(&p.channel_nonce);
        self.request(&b.request)?;
        self.byte(u8::from(b.gate.is_some()));
        if let Some(g) = &b.gate {
            self.byte(stage(g.stage));
            self.path(&g.path)?;
        }
        Ok(())
    }
    fn path(&mut self, path: &str) -> Result<(), CodecError> {
        if path.len() > 4096 || !crate::fixture::canonical_path(path) {
            return Err(CodecError::Value);
        }
        self.blob(path.as_bytes())
    }
}
impl read::Reader<'_> {
    pub(super) fn bootstrap(&mut self) -> Result<StorageBootstrap, CodecError> {
        let root = self.path()?;
        let source = SourceStamp {
            generation: self.id()?,
            root: self.source_file()?,
            database: self.source_file()?,
            marker: self.source_file()?,
            store_lock: self.source_file()?,
            io_lock: self.source_file()?,
        };
        let participant = StorageParticipant {
            source,
            operation: self.id()?,
            attempt: self.id()?,
            request_digest: self.digest()?,
            configuration_digest: self.digest()?,
            profile_digest: self.digest()?,
            owner: self.owner()?,
            writer: self.process()?,
            channel_nonce: self.id()?,
        };
        let request = self.request()?;
        if request.operation != participant.operation
            || request.attempt != participant.attempt
            || request.generation != participant.source.generation
            || request.digest != participant.request_digest
        {
            return Err(CodecError::Value);
        }
        let gate = if self.boolean()? {
            Some(Gate {
                stage: parse_stage(self.byte()?)?,
                path: self.path()?,
            })
        } else {
            None
        };
        Ok(StorageBootstrap {
            root,
            request,
            participant,
            gate,
        })
    }
    fn source_file(&mut self) -> Result<FileIdentity, CodecError> {
        self.file()
    }
    fn path(&mut self) -> Result<String, CodecError> {
        let raw = self.blob()?;
        let path = String::from_utf8(raw).map_err(|_| CodecError::Value)?;
        if path.len() > 4096 || !crate::fixture::canonical_path(&path) {
            return Err(CodecError::Value);
        }
        Ok(path)
    }
}
fn stage(s: Stage) -> u8 {
    match s {
        Stage::ParentBeforePrepare => 1,
        Stage::ParentAfterPrepare => 2,
        Stage::ParentBeforeSpawn => 3,
        Stage::ParentAfterOwnerSpawn => 4,
        Stage::OwnerAfterClaim => 5,
        Stage::OwnerAfterChildSpawn => 6,
        Stage::OwnerAfterObserved => 7,
        Stage::OwnerBeforeAck => 8,
        Stage::CaptureBeforeSync => 9,
        Stage::CaptureAfterSync => 10,
        Stage::CaptureAfterPublish => 11,
        Stage::CaptureBeforeTerminal => 12,
        Stage::CaptureAfterTerminal => 13,
    }
}
fn parse_stage(n: u8) -> Result<Stage, CodecError> {
    Ok(match n {
        1 => Stage::ParentBeforePrepare,
        2 => Stage::ParentAfterPrepare,
        3 => Stage::ParentBeforeSpawn,
        4 => Stage::ParentAfterOwnerSpawn,
        5 => Stage::OwnerAfterClaim,
        6 => Stage::OwnerAfterChildSpawn,
        7 => Stage::OwnerAfterObserved,
        8 => Stage::OwnerBeforeAck,
        9 => Stage::CaptureBeforeSync,
        10 => Stage::CaptureAfterSync,
        11 => Stage::CaptureAfterPublish,
        12 => Stage::CaptureBeforeTerminal,
        13 => Stage::CaptureAfterTerminal,
        _ => return Err(CodecError::Value),
    })
}
