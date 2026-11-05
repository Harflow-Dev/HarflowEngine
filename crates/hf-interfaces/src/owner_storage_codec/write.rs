use super::*;
use hf_domain::{capture::*, process::*};
pub(super) struct Writer(pub Vec<u8>);
impl Writer {
    pub(super) fn message(&mut self, m: &Message) -> Result<(), CodecError> {
        match m {
            Message::Command(c) => self.command(c),
            Message::Ack(a) => {
                self.u64(a.identity.sequence);
                self.digest(&a.identity.binding_digest)?;
                self.digest(&a.identity.command_digest)?;
                self.byte(match a.disposition {
                    AckDisposition::Committed => 0,
                    AckDisposition::Refused => 1,
                    AckDisposition::Unknown => 2,
                });
                self.byte(u8::from(a.revision.is_some()));
                if let Some(v) = a.revision {
                    self.u64(v);
                }
                self.byte(u8::from(a.snapshot.is_some()));
                if let Some(v) = &a.snapshot {
                    self.snapshot(v)?;
                }
                self.byte(u8::from(a.capture.is_some()));
                if let Some(v) = &a.capture {
                    self.capture(v);
                }
                self.byte(u8::from(a.raw_files.is_some()));
                if let Some(raw) = &a.raw_files {
                    if !raw.is_valid() {
                        return Err(CodecError::Value);
                    }
                    self.file(&raw.stdout);
                    self.file(&raw.stderr);
                }
                if a.disposition == AckDisposition::Committed && a.revision.is_none() {
                    return Err(CodecError::Value);
                }
                Ok(())
            }
        }
    }
    pub(super) fn command(&mut self, c: &StorageCommand) -> Result<(), CodecError> {
        match c {
            StorageCommand::Bootstrap(b) => {
                self.byte(0);
                self.bootstrap(b)?;
            }
            StorageCommand::Claim => self.byte(1),
            StorageCommand::RecordChild(p) => {
                self.byte(2);
                self.process(p);
            }
            StorageCommand::CreateRaw => self.byte(3),
            StorageCommand::Accept => self.byte(4),
            StorageCommand::CaptureBegin(b) => {
                self.byte(5);
                self.binding(b);
            }
            StorageCommand::Append {
                stream,
                bytes,
                drain,
            } => {
                if bytes.len() > 16384 || (drain.is_some() && !bytes.is_empty()) {
                    return Err(CodecError::Length);
                }
                self.byte(6);
                self.byte(match stream {
                    Stream::Stdout => 0,
                    Stream::Stderr => 1,
                });
                self.blob(bytes)?;
                self.byte(u8::from(drain.is_some()));
                if let Some(d) = drain {
                    for b in [
                        d.reader_joined,
                        d.accepted_data_reconciled,
                        d.input_reconciled,
                        d.stdout_eof,
                        d.stderr_eof,
                    ] {
                        self.byte(u8::from(b));
                    }
                }
            }
            StorageCommand::KnownLoss => self.byte(7),
            StorageCommand::Finish(e) => {
                self.byte(8);
                self.exit(*e);
            }
            StorageCommand::Close => self.byte(9),
        }
        Ok(())
    }
    pub(super) fn byte(&mut self, v: u8) {
        self.0.push(v);
    }
    pub(super) fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub(super) fn id(&mut self, v: &Id) {
        self.0.extend_from_slice(v.as_str().as_bytes());
    }
    pub(super) fn digest(&mut self, v: &Digest) -> Result<(), CodecError> {
        self.0.extend_from_slice(&raw_digest(v)?);
        Ok(())
    }
    pub(super) fn blob(&mut self, b: &[u8]) -> Result<(), CodecError> {
        if b.len() > 16384 {
            return Err(CodecError::Length);
        }
        self.0.extend_from_slice(&(b.len() as u32).to_be_bytes());
        self.0.extend_from_slice(b);
        Ok(())
    }
    pub(super) fn process(&mut self, p: &ProcessIdentity) {
        self.0.extend_from_slice(&p.pid().to_be_bytes());
        self.u64(p.start_seconds());
        self.u64(p.start_microseconds());
        self.0.extend_from_slice(p.boot_uuid().as_bytes());
    }
    pub(super) fn owner(&mut self, o: &OwnerClaim) {
        self.id(&o.owner);
        self.id(&o.nonce);
        self.process(&o.process);
    }
    pub(super) fn fence(&mut self, f: &Fence) {
        self.id(&f.operation);
        self.id(&f.generation);
        self.id(&f.attempt);
        self.owner(&f.owner);
        self.u64(f.revision);
    }
    pub(super) fn file(&mut self, f: &FileIdentity) {
        self.u64(f.device);
        self.u64(f.inode);
    }
    pub(super) fn binding(&mut self, b: &CaptureBinding) {
        self.fence(&b.fence);
        self.process(&b.child);
        self.file(&b.stdout);
        self.file(&b.stderr);
    }
    pub(super) fn exit(&mut self, e: ProcessExit) {
        match e {
            ProcessExit::Unobserved => self.byte(0),
            ProcessExit::Code(v) => {
                self.byte(1);
                self.0.extend_from_slice(&v.to_be_bytes());
            }
            ProcessExit::Signal(v) => {
                self.byte(2);
                self.0.extend_from_slice(&v.to_be_bytes());
            }
        }
    }
    pub(super) fn capture(&mut self, c: &CaptureSnapshot) {
        self.binding(&c.binding);
        for n in [c.progress.revision, c.progress.stdout, c.progress.stderr] {
            self.u64(n);
        }
        for b in [
            c.progress.stdout_eof,
            c.progress.stderr_eof,
            c.progress.loss,
        ] {
            self.byte(u8::from(b));
        }
        self.exit(c.exit);
    }
    pub(super) fn snapshot(&mut self, s: &Snapshot) -> Result<(), CodecError> {
        self.request(&s.request)?;
        self.byte(match s.phase {
            Phase::Prepared => 0,
            Phase::Claimed => 1,
            Phase::SpawnObserved => 2,
            Phase::AcceptanceRecorded => 3,
            Phase::Released => 4,
        });
        self.u64(s.revision);
        self.byte(u8::from(s.cancelled));
        self.byte(u8::from(s.owner.is_some()));
        if let Some(o) = &s.owner {
            self.owner(o);
        }
        self.byte(u8::from(s.child.is_some()));
        if let Some(p) = &s.child {
            self.process(p);
        }
        Ok(())
    }
    pub(super) fn request(&mut self, r: &PreparedRequest) -> Result<(), CodecError> {
        for id in [
            &r.operation,
            &r.session,
            &r.reservation,
            &r.attempt,
            &r.generation,
        ] {
            self.id(id);
        }
        self.digest(&r.digest)?;
        self.blob(&r.payload)?;
        Ok(())
    }
}
