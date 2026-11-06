use super::{CodecError, MAX_FRAME, MAX_PAYLOAD, MAX_ROOT, canonical_path};
use hf_domain::{fixture::*, ownership::*, process::ProcessIdentity};
pub(super) fn decode(bytes: &[u8]) -> Result<Frame, CodecError> {
    if bytes.len() > MAX_FRAME || bytes.len() < 10 {
        return Err(CodecError::Length);
    }
    let mut r = Reader { bytes, at: 0 };
    let n = r.u32()? as usize;
    if n != bytes.len() - 4 {
        return Err(CodecError::Length);
    }
    if r.take(5)? != b"HFB2\x01" {
        return Err(CodecError::Shape);
    }
    let frame = match r.byte()? {
        1 => Frame::Hello(r.process()?),
        10 => {
            let invitation = r.invitation()?;
            let body = r.blob(MAX_FRAME)?;
            Frame::CaptureAccepted(Box::new(
                crate::owner_storage_codec::decode_capture_receipt(invitation, &body)
                    .map_err(|_| CodecError::Value)?,
            ))
        }
        2 => Frame::Invitation(r.invitation()?),
        3 => Frame::Ready(r.invitation()?),
        4 => Frame::Prepared(r.prepared()?),
        5 => Frame::Accepted {
            invitation: r.invitation()?,
            fence: r.fence()?,
            child: r.process()?,
        },
        6 => Frame::Parent(ParentPlan {
            root: r.path()?,
            request: r.request()?,
            initialize: r.boolean()?,
            launch: r.boolean()?,
            owner: r.id()?,
            nonce: r.id()?,
            gate: r.gate()?,
            ledger: r.ledger()?,
            fault: r.fault()?,
        }),
        7 => Frame::Start(EffectStart {
            operation: r.id()?,
            root: r.path()?,
            ledger: r.ledger()?,
            fence: r.fence()?,
        }),
        8 => Frame::EffectDone {
            operation: r.id()?,
            process: r.process()?,
        },
        9 => Frame::Report {
            operation: r.id()?,
            generation: r.id()?,
            kind: match r.byte()? {
                0 => ReportKind::Prepared,
                1 => ReportKind::Existing,
                2 => ReportKind::Refused,
                3 => ReportKind::Uncertain,
                _ => return Err(CodecError::Shape),
            },
        },
        _ => return Err(CodecError::Shape),
    };
    if r.at != bytes.len() {
        return Err(CodecError::Shape);
    }
    Ok(frame)
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        let end = self.at.checked_add(n).ok_or(CodecError::Length)?;
        let value = self.bytes.get(self.at..end).ok_or(CodecError::Length)?;
        self.at = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| CodecError::Length)?,
        ))
    }
    fn u64(&mut self) -> Result<u64, CodecError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| CodecError::Length)?,
        ))
    }
    fn text(&mut self, n: usize) -> Result<String, CodecError> {
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| CodecError::Value)
    }
    fn id(&mut self) -> Result<Id, CodecError> {
        let s = self.text(32)?;
        if s.bytes().any(|b| b.is_ascii_uppercase()) {
            return Err(CodecError::Value);
        }
        Id::new(s).map_err(|_| CodecError::Value)
    }
    fn digest(&mut self) -> Result<Digest, CodecError> {
        let s = self.text(64)?;
        if s.bytes().any(|b| b.is_ascii_uppercase()) {
            return Err(CodecError::Value);
        }
        Digest::new(s).map_err(|_| CodecError::Value)
    }
    fn blob(&mut self, max: usize) -> Result<Vec<u8>, CodecError> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(CodecError::Length);
        }
        Ok(self.take(n)?.to_vec())
    }
    fn path(&mut self) -> Result<String, CodecError> {
        let s = String::from_utf8(self.blob(MAX_ROOT)?).map_err(|_| CodecError::Value)?;
        if !canonical_path(&s) {
            return Err(CodecError::Value);
        }
        Ok(s)
    }
    fn process(&mut self) -> Result<ProcessIdentity, CodecError> {
        let (pid, secs, micros) = (self.u32()?, self.u64()?, self.u64()?);
        let boot = self.text(36)?;
        if boot.bytes().any(|b| b.is_ascii_uppercase()) {
            return Err(CodecError::Value);
        }
        ProcessIdentity::new(pid, secs, micros, boot).map_err(|_| CodecError::Value)
    }
    fn invitation(&mut self) -> Result<Invitation, CodecError> {
        Ok(Invitation {
            operation: self.id()?,
            generation: self.id()?,
            digest: self.digest()?,
            owner: self.id()?,
            nonce: self.id()?,
            process: self.process()?,
            revision: self.u64()?,
        })
    }
    fn fence(&mut self) -> Result<Fence, CodecError> {
        Ok(Fence {
            operation: self.id()?,
            generation: self.id()?,
            attempt: self.id()?,
            owner: OwnerClaim {
                owner: self.id()?,
                nonce: self.id()?,
                process: self.process()?,
            },
            revision: self.u64()?,
        })
    }
    fn prepared(&mut self) -> Result<Prepared, CodecError> {
        let invitation = self.invitation()?;
        let request = self.request()?;
        let root = self.path()?;
        let gate = self.gate()?;
        let ledger = self.ledger()?;
        let fault = self.fault()?;
        Ok(Prepared {
            invitation,
            request,
            root,
            gate,
            ledger,
            fault,
        })
    }
    fn request(&mut self) -> Result<PreparedRequest, CodecError> {
        Ok(PreparedRequest {
            operation: self.id()?,
            session: self.id()?,
            reservation: self.id()?,
            attempt: self.id()?,
            generation: self.id()?,
            digest: self.digest()?,
            payload: self.blob(MAX_PAYLOAD)?,
        })
    }
    fn boolean(&mut self) -> Result<bool, CodecError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::Shape),
        }
    }
    fn ledger(&mut self) -> Result<QualifiedLedger, CodecError> {
        Ok(QualifiedLedger {
            device: self.u64()?,
            inode: self.u64()?,
        })
    }
    fn fault(&mut self) -> Result<FixtureFault, CodecError> {
        match self.byte()? {
            0 => Ok(FixtureFault::None),
            1 => Ok(FixtureFault::UnknownChildIdentity),
            2 => Ok(FixtureFault::UnusableChildPipe),
            3 => Ok(FixtureFault::OwnerCapacityExhausted),
            4 => Ok(FixtureFault::OutputSaturation),
            _ => Err(CodecError::Shape),
        }
    }
    fn gate(&mut self) -> Result<Option<Gate>, CodecError> {
        Ok(match self.byte()? {
            0 => None,
            1 => Some(Gate {
                stage: self.stage()?,
                path: self.path()?,
            }),
            _ => return Err(CodecError::Shape),
        })
    }
    fn stage(&mut self) -> Result<Stage, CodecError> {
        match self.byte()? {
            1 => Ok(Stage::ParentBeforePrepare),
            2 => Ok(Stage::ParentAfterPrepare),
            3 => Ok(Stage::ParentBeforeSpawn),
            4 => Ok(Stage::ParentAfterOwnerSpawn),
            5 => Ok(Stage::OwnerAfterClaim),
            6 => Ok(Stage::OwnerAfterChildSpawn),
            7 => Ok(Stage::OwnerAfterObserved),
            8 => Ok(Stage::OwnerBeforeAck),
            9 => Ok(Stage::CaptureBeforeSync),
            10 => Ok(Stage::CaptureAfterSync),
            11 => Ok(Stage::CaptureAfterPublish),
            12 => Ok(Stage::CaptureBeforeTerminal),
            13 => Ok(Stage::CaptureAfterTerminal),
            _ => Err(CodecError::Shape),
        }
    }
}
