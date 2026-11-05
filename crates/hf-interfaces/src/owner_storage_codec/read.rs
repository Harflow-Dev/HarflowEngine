use super::*;
use hf_domain::{capture::*, process::*};
pub(super) struct Reader<'a> {
    pub bytes: &'a [u8],
    pub at: usize,
}
impl<'a> Reader<'a> {
    pub(super) fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        let end = self.at.checked_add(n).ok_or(CodecError::Length)?;
        let b = self.bytes.get(self.at..end).ok_or(CodecError::Length)?;
        self.at = end;
        Ok(b)
    }
    pub(super) fn byte(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }
    pub(super) fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| CodecError::Length)?,
        ))
    }
    pub(super) fn u64(&mut self) -> Result<u64, CodecError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| CodecError::Length)?,
        ))
    }
    pub(super) fn digest(&mut self) -> Result<Digest, CodecError> {
        Ok(Digest::from_sha256_bytes(
            self.take(32)?.try_into().map_err(|_| CodecError::Length)?,
        ))
    }
    pub(super) fn boolean(&mut self) -> Result<bool, CodecError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::Value),
        }
    }
    pub(super) fn id(&mut self) -> Result<Id, CodecError> {
        let b = self.take(32)?;
        if !b
            .iter()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(v))
        {
            return Err(CodecError::Value);
        }
        Id::new(String::from_utf8(b.to_vec()).map_err(|_| CodecError::Value)?)
            .map_err(|_| CodecError::Value)
    }
    pub(super) fn blob(&mut self) -> Result<Vec<u8>, CodecError> {
        let n = self.u32()? as usize;
        if n > 16384 {
            return Err(CodecError::Length);
        }
        Ok(self.take(n)?.to_vec())
    }
    pub(super) fn process(&mut self) -> Result<ProcessIdentity, CodecError> {
        let (pid, secs, micros) = (self.u32()?, self.u64()?, self.u64()?);
        let b = self.take(36)?;
        if b.iter().any(u8::is_ascii_uppercase) {
            return Err(CodecError::Value);
        }
        ProcessIdentity::new(
            pid,
            secs,
            micros,
            String::from_utf8(b.to_vec()).map_err(|_| CodecError::Value)?,
        )
        .map_err(|_| CodecError::Value)
    }
    pub(super) fn owner(&mut self) -> Result<OwnerClaim, CodecError> {
        Ok(OwnerClaim {
            owner: self.id()?,
            nonce: self.id()?,
            process: self.process()?,
        })
    }
    pub(super) fn fence(&mut self) -> Result<Fence, CodecError> {
        Ok(Fence {
            operation: self.id()?,
            generation: self.id()?,
            attempt: self.id()?,
            owner: self.owner()?,
            revision: self.u64()?,
        })
    }
    pub(super) fn file(&mut self) -> Result<FileIdentity, CodecError> {
        let v = FileIdentity {
            device: self.u64()?,
            inode: self.u64()?,
        };
        if v.inode == 0 {
            return Err(CodecError::Value);
        }
        Ok(v)
    }
    pub(super) fn binding(&mut self) -> Result<CaptureBinding, CodecError> {
        Ok(CaptureBinding {
            fence: self.fence()?,
            child: self.process()?,
            stdout: self.file()?,
            stderr: self.file()?,
        })
    }
    pub(super) fn exit(&mut self) -> Result<ProcessExit, CodecError> {
        match self.byte()? {
            0 => Ok(ProcessExit::Unobserved),
            tag @ (1 | 2) => {
                let n =
                    i32::from_be_bytes(self.take(4)?.try_into().map_err(|_| CodecError::Length)?);
                if tag == 1 {
                    Ok(ProcessExit::Code(n))
                } else if n > 0 {
                    Ok(ProcessExit::Signal(n))
                } else {
                    Err(CodecError::Value)
                }
            }
            _ => Err(CodecError::Value),
        }
    }
    pub(super) fn capture(&mut self) -> Result<CaptureSnapshot, CodecError> {
        let binding = self.binding()?;
        let progress = CaptureProgress {
            revision: self.u64()?,
            stdout: self.u64()?,
            stderr: self.u64()?,
            stdout_eof: self.boolean()?,
            stderr_eof: self.boolean()?,
            loss: self.boolean()?,
        };
        if progress.revision > 128 || progress.stdout > 65536 || progress.stderr > 65536 {
            return Err(CodecError::Value);
        }
        Ok(CaptureSnapshot {
            binding,
            progress,
            exit: self.exit()?,
        })
    }
    pub(super) fn snapshot(&mut self) -> Result<Snapshot, CodecError> {
        let request = self.request()?;
        let phase = match self.byte()? {
            0 => Phase::Prepared,
            1 => Phase::Claimed,
            2 => Phase::SpawnObserved,
            3 => Phase::AcceptanceRecorded,
            4 => Phase::Released,
            _ => return Err(CodecError::Value),
        };
        let revision = self.u64()?;
        let cancelled = self.boolean()?;
        let owner = if self.boolean()? {
            Some(self.owner()?)
        } else {
            None
        };
        let child = if self.boolean()? {
            Some(self.process()?)
        } else {
            None
        };
        Ok(Snapshot {
            request,
            phase,
            revision,
            cancelled,
            owner,
            child,
        })
    }
    pub(super) fn message(&mut self, tag: u8) -> Result<Message, CodecError> {
        match tag {
            1 => Ok(Message::Command(self.command()?)),
            2 => {
                let identity = CommandIdentity {
                    sequence: self.u64()?,
                    binding_digest: self.digest()?,
                    command_digest: self.digest()?,
                };
                let disposition = match self.byte()? {
                    0 => AckDisposition::Committed,
                    1 => AckDisposition::Refused,
                    2 => AckDisposition::Unknown,
                    _ => return Err(CodecError::Value),
                };
                let revision = if self.boolean()? {
                    Some(self.u64()?)
                } else {
                    None
                };
                if disposition == AckDisposition::Committed && revision.is_none() {
                    return Err(CodecError::Value);
                }
                let snapshot = if self.boolean()? {
                    Some(self.snapshot()?)
                } else {
                    None
                };
                let capture = if self.boolean()? {
                    Some(self.capture()?)
                } else {
                    None
                };
                let raw_files = if self.boolean()? {
                    let raw = RawIdentities {
                        stdout: self.file()?,
                        stderr: self.file()?,
                    };
                    if !raw.is_valid() {
                        return Err(CodecError::Value);
                    }
                    Some(raw)
                } else {
                    None
                };
                Ok(Message::Ack(Box::new(StorageAck {
                    identity,
                    disposition,
                    revision,
                    snapshot,
                    capture,
                    raw_files,
                })))
            }
            _ => Err(CodecError::Shape),
        }
    }
    pub(super) fn command(&mut self) -> Result<StorageCommand, CodecError> {
        Ok(match self.byte()? {
            0 => StorageCommand::Bootstrap(Box::new(self.bootstrap()?)),
            1 => StorageCommand::Claim,
            2 => StorageCommand::RecordChild(self.process()?),
            3 => StorageCommand::CreateRaw,
            4 => StorageCommand::Accept,
            5 => StorageCommand::CaptureBegin(Box::new(self.binding()?)),
            6 => {
                let stream = match self.byte()? {
                    0 => Stream::Stdout,
                    1 => Stream::Stderr,
                    _ => return Err(CodecError::Value),
                };
                let bytes = self.blob()?;
                let drain = if self.boolean()? {
                    Some(TrustedDrainEvidence {
                        reader_joined: self.boolean()?,
                        accepted_data_reconciled: self.boolean()?,
                        input_reconciled: self.boolean()?,
                        stdout_eof: self.boolean()?,
                        stderr_eof: self.boolean()?,
                    })
                } else {
                    None
                };
                if drain.is_some() && !bytes.is_empty() {
                    return Err(CodecError::Value);
                }
                StorageCommand::Append {
                    stream,
                    bytes,
                    drain,
                }
            }
            7 => StorageCommand::KnownLoss,
            8 => StorageCommand::Finish(self.exit()?),
            9 => StorageCommand::Close,
            _ => return Err(CodecError::Value),
        })
    }
    pub(super) fn request(&mut self) -> Result<PreparedRequest, CodecError> {
        Ok(PreparedRequest {
            operation: self.id()?,
            session: self.id()?,
            reservation: self.id()?,
            attempt: self.id()?,
            generation: self.id()?,
            digest: self.digest()?,
            payload: self.blob()?,
        })
    }
}
