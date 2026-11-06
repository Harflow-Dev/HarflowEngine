//! Bounded HFCQ1 framing. Semantic command/profile checks belong to composition.
use crate::{
    owner_preparation::{self, PreparationError},
    prepared_launch::wire::{Reader, identity, number, text},
};
use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    claude_qualification::*,
    fixture::QualifiedLedger,
    ownership::{Digest, PreparedRequest},
    prepared_launch::{BinaryMetadata, BinaryPin},
    recovery::SourceStamp,
};
mod validation;
pub use validation::check;
pub fn configuration_digest(
    v: &ClaudeQualification,
    hash: &impl DigestPort,
) -> Result<Digest, PreparationError> {
    check(v)?;
    let mut policy = v.policy.clone();
    policy.configuration_digest = Digest::from_sha256_bytes([0; 32]);
    Ok(hash.digest_parts(&[
        b"HFCQ-config\x01",
        &body(v, &owner_preparation::encode(&policy)?)?,
    ]))
}
pub fn encode(
    v: &ClaudeQualification,
    hash: &impl DigestPort,
) -> Result<Vec<u8>, PreparationError> {
    check(v)?;
    if configuration_digest(v, hash)? != v.policy.configuration_digest
        || hash.digest_parts(&[&v.profile]) != v.profile_digest
    {
        return Err(PreparationError::Binding);
    }
    body(v, &owner_preparation::encode(&v.policy)?)
}
fn body(v: &ClaudeQualification, policy: &[u8]) -> Result<Vec<u8>, PreparationError> {
    let mut w = b"HFCQ\x01".to_vec();
    w.extend_from_slice(&(policy.len() as u32).to_be_bytes());
    w.extend_from_slice(policy);
    pin(&mut w, &v.gate_binary)?;
    pin(&mut w, &v.vendor_binary)?;
    text(&mut w, &v.logical_hint)?;
    text(&mut w, &v.vendor_version)?;
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
    directory(&mut w, &v.topology.root)?;
    number(&mut w, v.topology.roles.len() as u64);
    for d in &v.topology.roles {
        directory(&mut w, d)?;
    }
    text(&mut w, &v.nonce)?;
    number(&mut w, u64::from(v.port));
    text(
        &mut w,
        std::str::from_utf8(&v.profile).map_err(|_| PreparationError::Shape)?,
    )?;
    w.extend_from_slice(v.profile_digest.as_str().as_bytes());
    text(&mut w, &v.command.executable)?;
    number(&mut w, v.command.args.len() as u64);
    for a in &v.command.args {
        text(&mut w, a)?;
    }
    number(&mut w, v.command.environment.len() as u64);
    for (k, val) in &v.command.environment {
        text(&mut w, k)?;
        text(&mut w, val)?;
    }
    if w.len() > owner_preparation::MAX_PAYLOAD {
        return Err(PreparationError::Shape);
    }
    Ok(w)
}
fn pin(w: &mut Vec<u8>, p: &BinaryPin) -> Result<(), PreparationError> {
    text(w, &p.logical_path)?;
    text(w, &p.physical_path)?;
    identity(w, &p.metadata.identity);
    for n in [
        p.metadata.length,
        p.metadata.modified_seconds as u64,
        p.metadata.modified_nanoseconds as u64,
        p.metadata.changed_seconds as u64,
        p.metadata.changed_nanoseconds as u64,
    ] {
        number(w, n);
    }
    w.extend_from_slice(p.digest.as_str().as_bytes());
    Ok(())
}
fn directory(w: &mut Vec<u8>, d: &RecordedDirectory) -> Result<(), PreparationError> {
    text(w, &d.path)?;
    identity(w, &d.identity);
    Ok(())
}
pub fn decode(
    request: &PreparedRequest,
    hash: &impl DigestPort,
) -> Result<ClaudeQualification, PreparationError> {
    if request.payload.len() > owner_preparation::MAX_PAYLOAD
        || hash.digest_parts(&[&request.payload]) != request.digest
    {
        return Err(PreparationError::Binding);
    }
    let mut r = Reader {
        bytes: &request.payload,
        at: 0,
    };
    if r.take(5)? != b"HFCQ\x01" {
        return Err(PreparationError::Shape);
    }
    let n = r.u32()? as usize;
    let policy_bytes = r.take(n)?;
    let mut embedded = request.clone();
    embedded.payload = policy_bytes.to_vec();
    embedded.digest = hash.digest_parts(&[policy_bytes]);
    let policy = owner_preparation::decode(&embedded, hash)?;
    let v = read_fields(policy, &mut r)?;
    if r.at != request.payload.len() {
        return Err(PreparationError::Shape);
    }
    check(&v)?;
    if configuration_digest(&v, hash)? != v.policy.configuration_digest
        || hash.digest_parts(&[&v.profile]) != v.profile_digest
    {
        return Err(PreparationError::Binding);
    }
    Ok(v)
}
fn read_fields(
    policy: hf_domain::owner_policy::PreparationEnvelope,
    r: &mut Reader<'_>,
) -> Result<ClaudeQualification, PreparationError> {
    let gate_binary = read_pin(r)?;
    let vendor_binary = read_pin(r)?;
    let logical_hint = r.text()?;
    let vendor_version = r.text()?;
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
    let root = read_directory(r)?;
    let n = count(r, 6)?;
    let mut roles = Vec::new();
    for _ in 0..n {
        roles.push(read_directory(r)?);
    }
    let nonce = r.text()?;
    let port = u16::try_from(r.number()?).map_err(|_| PreparationError::Shape)?;
    let profile = r.text()?.into_bytes();
    let profile_digest = r.digest()?;
    let executable = r.text()?;
    let n = count(r, 40)?;
    let mut args = Vec::new();
    for _ in 0..n {
        args.push(r.text()?);
    }
    let n = count(r, 24)?;
    let mut environment = Vec::new();
    for _ in 0..n {
        environment.push((r.text()?, r.text()?));
    }
    Ok(ClaudeQualification {
        policy,
        gate_binary,
        vendor_binary,
        vendor_version,
        logical_hint,
        cwd,
        source,
        ledger,
        topology: RecordedFixtureTopology { root, roles },
        nonce,
        port,
        profile,
        profile_digest,
        command: QualificationCommand {
            executable,
            args,
            environment,
        },
    })
}
fn count(r: &mut Reader<'_>, max: u64) -> Result<u64, PreparationError> {
    let n = r.number()?;
    if n > max {
        return Err(PreparationError::Shape);
    }
    Ok(n)
}
fn read_directory(r: &mut Reader<'_>) -> Result<RecordedDirectory, PreparationError> {
    Ok(RecordedDirectory {
        path: r.text()?,
        identity: r.identity()?,
    })
}
fn read_pin(r: &mut Reader<'_>) -> Result<BinaryPin, PreparationError> {
    Ok(BinaryPin {
        logical_path: r.text()?,
        physical_path: r.text()?,
        metadata: BinaryMetadata {
            identity: r.identity()?,
            length: r.number()?,
            modified_seconds: r.number()? as i64,
            modified_nanoseconds: r.number()? as i64,
            changed_seconds: r.number()? as i64,
            changed_nanoseconds: r.number()? as i64,
        },
        digest: r.digest()?,
    })
}
