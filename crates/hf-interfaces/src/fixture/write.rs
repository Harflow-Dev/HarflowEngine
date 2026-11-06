use super::{CodecError, MAX_FRAME, MAX_PAYLOAD, canonical_path};
use hf_domain::{fixture::*, ownership::*, process::ProcessIdentity};
pub(super) fn encode(frame: &Frame) -> Result<Vec<u8>, CodecError> {
    let mut w = Writer(Vec::new());
    w.0.extend_from_slice(b"HFB2\x01");
    match frame {
        Frame::CaptureAccepted(v) => {
            w.receipt(v)?;
        }
        Frame::Hello(p) => {
            w.0.push(1);
            w.process(p);
        }
        Frame::Invitation(v) => {
            w.0.push(2);
            w.invitation(v);
        }
        Frame::Ready(v) => {
            w.0.push(3);
            w.invitation(v);
        }
        Frame::Prepared(v) => {
            w.0.push(4);
            w.prepared(v)?;
        }
        Frame::Accepted {
            invitation,
            fence,
            child,
        } => {
            w.0.push(5);
            w.invitation(invitation);
            w.fence(fence);
            w.process(child);
        }
        Frame::Parent(v) => {
            w.0.push(6);
            w.path(&v.root)?;
            w.request(&v.request)?;
            w.0.extend_from_slice(&[u8::from(v.initialize), u8::from(v.launch)]);
            w.id(&v.owner);
            w.id(&v.nonce);
            w.gate(&v.gate)?;
            w.ledger(v.ledger);
            w.fault(v.fault);
        }
        Frame::Start(v) => {
            w.0.push(7);
            w.id(&v.operation);
            w.path(&v.root)?;
            w.ledger(v.ledger);
            w.fence(&v.fence);
        }
        Frame::LaunchReady {
            operation,
            attempt,
            request_digest,
            child,
        } => {
            w.0.push(11);
            w.id(operation);
            w.id(attempt);
            w.0.extend_from_slice(request_digest.as_str().as_bytes());
            w.process(child);
        }
        Frame::EffectDone { operation, process } => {
            w.0.push(8);
            w.id(operation);
            w.process(process);
        }
        Frame::Report {
            operation,
            generation,
            kind,
        } => w.report(operation, generation, *kind),
    }
    if w.0.len() + 4 > MAX_FRAME {
        return Err(CodecError::Length);
    }
    let length = u32::try_from(w.0.len()).map_err(|_| CodecError::Length)?;
    let mut bytes = length.to_be_bytes().to_vec();
    bytes.extend(w.0);
    Ok(bytes)
}
struct Writer(Vec<u8>);
impl Writer {
    fn report(&mut self, operation: &Id, generation: &Id, kind: ReportKind) {
        self.0.push(9);
        self.id(operation);
        self.id(generation);
        self.0.push(match kind {
            ReportKind::Prepared => 0,
            ReportKind::Existing => 1,
            ReportKind::Refused => 2,
            ReportKind::Uncertain => 3,
        });
    }
    fn receipt(
        &mut self,
        v: &hf_domain::prepared_launch::CaptureAccepted,
    ) -> Result<(), CodecError> {
        self.0.push(10);
        self.invitation(&v.invitation);
        self.blob(
            &crate::owner_storage_codec::encode_capture_receipt(v)
                .map_err(|_| CodecError::Value)?,
        )
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn id(&mut self, v: &Id) {
        self.0.extend_from_slice(v.as_str().as_bytes());
    }
    fn blob(&mut self, v: &[u8]) -> Result<(), CodecError> {
        let n = u32::try_from(v.len()).map_err(|_| CodecError::Length)?;
        self.0.extend_from_slice(&n.to_be_bytes());
        self.0.extend_from_slice(v);
        Ok(())
    }
    fn process(&mut self, p: &ProcessIdentity) {
        self.0.extend_from_slice(&p.pid().to_be_bytes());
        self.u64(p.start_seconds());
        self.u64(p.start_microseconds());
        self.0.extend_from_slice(p.boot_uuid().as_bytes());
    }
    fn invitation(&mut self, v: &Invitation) {
        self.id(&v.operation);
        self.id(&v.generation);
        self.0.extend_from_slice(v.digest.as_str().as_bytes());
        self.id(&v.owner);
        self.id(&v.nonce);
        self.process(&v.process);
        self.u64(v.revision);
    }
    fn fence(&mut self, f: &Fence) {
        self.id(&f.operation);
        self.id(&f.generation);
        self.id(&f.attempt);
        self.id(&f.owner.owner);
        self.id(&f.owner.nonce);
        self.process(&f.owner.process);
        self.u64(f.revision);
    }
    fn prepared(&mut self, v: &Prepared) -> Result<(), CodecError> {
        self.invitation(&v.invitation);
        self.request(&v.request)?;
        self.path(&v.root)?;
        self.gate(&v.gate)?;
        self.ledger(v.ledger);
        self.fault(v.fault);
        Ok(())
    }
    fn request(&mut self, r: &PreparedRequest) -> Result<(), CodecError> {
        if r.payload.len() > MAX_PAYLOAD {
            return Err(CodecError::Value);
        }
        for id in [
            &r.operation,
            &r.session,
            &r.reservation,
            &r.attempt,
            &r.generation,
        ] {
            self.id(id);
        }
        self.0.extend_from_slice(r.digest.as_str().as_bytes());
        self.blob(&r.payload)?;
        Ok(())
    }
    fn path(&mut self, path: &str) -> Result<(), CodecError> {
        if !canonical_path(path) {
            return Err(CodecError::Value);
        }
        self.blob(path.as_bytes())
    }
    fn ledger(&mut self, ledger: QualifiedLedger) {
        self.u64(ledger.device);
        self.u64(ledger.inode);
    }
    fn fault(&mut self, fault: FixtureFault) {
        self.0.push(match fault {
            FixtureFault::None => 0,
            FixtureFault::UnknownChildIdentity => 1,
            FixtureFault::UnusableChildPipe => 2,
            FixtureFault::OwnerCapacityExhausted => 3,
            FixtureFault::OutputSaturation => 4,
        });
    }
    fn gate(&mut self, gate: &Option<Gate>) -> Result<(), CodecError> {
        match gate {
            None => self.0.push(0),
            Some(g) => {
                if !canonical_path(&g.path) {
                    return Err(CodecError::Value);
                }
                self.0.push(1);
                self.0.push(stage(g.stage));
                self.blob(g.path.as_bytes())?;
            }
        }
        Ok(())
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
