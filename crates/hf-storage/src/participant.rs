//! Immutable deny-only participant evidence; this file never grants control.
#[cfg(unix)]
use crate::connection;
use crate::{Error, Result, Store};
#[cfg(unix)]
use hf_domain::process::ProcessIdentity;
use hf_domain::{capture::FileIdentity, owner_runtime::StorageParticipant};
use std::time::Instant;
#[cfg(unix)]
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParticipantIdentity {
    pub file: FileIdentity,
}
impl Store {
    #[cfg(not(unix))]
    pub fn create_storage_participant(
        &self,
        _: &StorageParticipant,
        _: Instant,
    ) -> Result<ParticipantIdentity> {
        Err(Error::InvalidRoot)
    }
    #[cfg(unix)]
    pub fn create_storage_participant(
        &self,
        participant: &StorageParticipant,
        deadline: Instant,
    ) -> Result<ParticipantIdentity> {
        connection::check_deadline(deadline)?;
        let source = self
            .binding
            .source_stamp(participant.source.generation.clone())?;
        if source != participant.source || source.generation.as_str() != self.identity.generation {
            return Err(Error::Replaced);
        }
        let bytes = encode(participant);
        if bytes.len() > 4096 {
            return Err(Error::LimitExceeded);
        }
        let path = self.binding.root().join(format!(
            "owner-participant-{}.v1",
            participant.operation.as_str()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .mode(0o600)
            .create_new(true)
            .open(&path)?;
        let before = file.metadata()?;
        if !before.is_file() || before.nlink() != 1 {
            return Err(Error::InvalidRoot);
        }
        file.write_all(&bytes)?;
        file.sync_all()?;
        let after = fs::symlink_metadata(&path)?;
        if after.file_type().is_symlink()
            || !after.is_file()
            || after.nlink() != 1
            || after.dev() != before.dev()
            || after.ino() != before.ino()
            || after.len() != bytes.len() as u64
        {
            return Err(Error::Replaced);
        }
        File::open(self.binding.root())?.sync_all()?;
        if self.binding.source_stamp(source.generation.clone())? != source {
            return Err(Error::Replaced);
        }
        connection::check_deadline(deadline)?;
        Ok(ParticipantIdentity {
            file: FileIdentity {
                device: after.dev(),
                inode: after.ino(),
            },
        })
    }
}
#[cfg(unix)]
fn process(value: &ProcessIdentity) -> String {
    format!(
        "{}:{}:{}:{}",
        value.pid(),
        value.start_seconds(),
        value.start_microseconds(),
        value.boot_uuid()
    )
}
/// Fixed positional UTF-8 lines; all text values are validated hex identifiers.
#[cfg(unix)]
fn encode(p: &StorageParticipant) -> Vec<u8> {
    let s = &p.source;
    let mut text = format!(
        "harflow-owner-participant-1\n{}\n{}\n{}\n",
        s.generation.as_str(),
        p.operation.as_str(),
        p.attempt.as_str()
    );
    for file in [&s.root, &s.database, &s.marker, &s.store_lock, &s.io_lock] {
        text.push_str(&format!("{}:{}\n", file.device, file.inode));
    }
    for value in [
        p.request_digest.as_str(),
        p.configuration_digest.as_str(),
        p.profile_digest.as_str(),
        p.owner.owner.as_str(),
        p.owner.nonce.as_str(),
    ] {
        text.push_str(value);
        text.push('\n');
    }
    text.push_str(&format!(
        "{}\n{}\n{}\n",
        process(&p.owner.process),
        process(&p.writer),
        p.channel_nonce.as_str()
    ));
    text.into_bytes()
}
