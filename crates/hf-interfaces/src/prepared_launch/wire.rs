use super::*;
use hf_domain::{capture::FileIdentity, fixture::QualifiedLedger, recovery::SourceStamp};
pub(super) fn fields(v: &PreparedLaunch) -> Result<Vec<u8>, PreparationError> {
    let mut w = Vec::new();
    w.push(0);
    for s in [&v.binary.logical_path, &v.binary.physical_path] {
        text(&mut w, s)?;
    }
    let m = &v.binary.metadata;
    identity(&mut w, &m.identity);
    for n in [
        m.length,
        m.modified_seconds as u64,
        m.modified_nanoseconds as u64,
        m.changed_seconds as u64,
        m.changed_nanoseconds as u64,
    ] {
        number(&mut w, n);
    }
    w.extend_from_slice(v.binary.digest.as_str().as_bytes());
    number(&mut w, v.argv.len() as u64);
    for s in &v.argv {
        text(&mut w, s)?;
    }
    number(&mut w, v.environment.len() as u64);
    for (k, val) in &v.environment {
        text(&mut w, k)?;
        text(&mut w, val)?;
    }
    text(&mut w, &v.cwd)?;
    w.extend_from_slice(v.source.generation.as_str().as_bytes());
    for i in [
        &v.source.root,
        &v.source.database,
        &v.source.marker,
        &v.source.store_lock,
        &v.source.io_lock,
    ] {
        identity(&mut w, i);
    }
    number(&mut w, v.ledger.device);
    number(&mut w, v.ledger.inode);
    text(&mut w, &v.source_profile)?;
    Ok(w)
}
pub(crate) fn number(w: &mut Vec<u8>, n: u64) {
    w.extend_from_slice(&n.to_be_bytes());
}
pub(crate) fn identity(w: &mut Vec<u8>, i: &FileIdentity) {
    number(w, i.device);
    number(w, i.inode);
}
pub(crate) fn text(w: &mut Vec<u8>, s: &str) -> Result<(), PreparationError> {
    if s.len() > 4096 || s.contains('\0') {
        return Err(PreparationError::Shape);
    }
    w.extend_from_slice(&(s.len() as u32).to_be_bytes());
    w.extend_from_slice(s.as_bytes());
    Ok(())
}
pub(super) fn split(bytes: &[u8]) -> Result<(&[u8], &[u8]), PreparationError> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(5)? != b"HFPL\x01" {
        return Err(PreparationError::Shape);
    }
    let n = r.u32()? as usize;
    let policy = r.take(n)?;
    Ok((policy, &bytes[r.at..]))
}
pub(super) fn decode(
    policy: PreparationEnvelope,
    bytes: &[u8],
) -> Result<PreparedLaunch, PreparationError> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(1)? != [0] {
        return Err(PreparationError::Shape);
    }
    let logical_path = r.text()?;
    let physical_path = r.text()?;
    let metadata = BinaryMetadata {
        identity: r.identity()?,
        length: r.number()?,
        modified_seconds: r.number()? as i64,
        modified_nanoseconds: r.number()? as i64,
        changed_seconds: r.number()? as i64,
        changed_nanoseconds: r.number()? as i64,
    };
    let digest = r.digest()?;
    if r.number()? != 1 {
        return Err(PreparationError::Shape);
    }
    let argv = vec![r.text()?];
    if r.number()? != 0 {
        return Err(PreparationError::Shape);
    }
    let cwd = r.text()?;
    let source = SourceStamp {
        generation: r.id()?,
        root: r.identity()?,
        database: r.identity()?,
        marker: r.identity()?,
        store_lock: r.identity()?,
        io_lock: r.identity()?,
    };
    let ledger = QualifiedLedger {
        device: r.number()?,
        inode: r.number()?,
    };
    let source_profile = r.text()?;
    if r.at != bytes.len() {
        return Err(PreparationError::Shape);
    }
    Ok(PreparedLaunch {
        policy,
        assignment: LaunchAssignment::FixedProtocolHelper,
        binary: BinaryPin {
            logical_path,
            physical_path,
            metadata,
            digest,
        },
        argv,
        environment: Vec::new(),
        cwd,
        source,
        ledger,
        source_profile,
    })
}
pub(crate) struct Reader<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) at: usize,
}
impl<'a> Reader<'a> {
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], PreparationError> {
        let end = self.at.checked_add(n).ok_or(PreparationError::Shape)?;
        let b = self
            .bytes
            .get(self.at..end)
            .ok_or(PreparationError::Shape)?;
        self.at = end;
        Ok(b)
    }
    pub(crate) fn u32(&mut self) -> Result<u32, PreparationError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| PreparationError::Shape)?,
        ))
    }
    pub(crate) fn number(&mut self) -> Result<u64, PreparationError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| PreparationError::Shape)?,
        ))
    }
    pub(crate) fn text(&mut self) -> Result<String, PreparationError> {
        let n = self.u32()? as usize;
        if n > 4096 {
            return Err(PreparationError::Shape);
        }
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| PreparationError::Shape)
    }
    pub(crate) fn fixed(&mut self, n: usize) -> Result<String, PreparationError> {
        let s = String::from_utf8(self.take(n)?.to_vec()).map_err(|_| PreparationError::Shape)?;
        if !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(PreparationError::Shape);
        }
        Ok(s)
    }
    pub(crate) fn id(&mut self) -> Result<Id, PreparationError> {
        Id::new(self.fixed(32)?).map_err(|_| PreparationError::Shape)
    }
    pub(crate) fn digest(&mut self) -> Result<Digest, PreparationError> {
        Digest::new(self.fixed(64)?).map_err(|_| PreparationError::Shape)
    }
    pub(crate) fn identity(&mut self) -> Result<FileIdentity, PreparationError> {
        Ok(FileIdentity {
            device: self.number()?,
            inode: self.number()?,
        })
    }
}
