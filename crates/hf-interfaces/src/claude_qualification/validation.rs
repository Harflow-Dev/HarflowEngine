use super::*;
pub fn check(v: &ClaudeQualification) -> Result<(), PreparationError> {
    if v.topology.roles.len() != 6
        || v.command.args.len() > 40
        || v.command.environment.len() > 24
        || v.profile.len() > 4096
        || v.profile.is_empty()
        || v.nonce.len() < 16
        || v.nonce.len() > 64
        || v.vendor_version.len() > 32
        || v.port == 0
    {
        return Err(PreparationError::Shape);
    }
    for s in v
        .command
        .args
        .iter()
        .chain(v.command.environment.iter().flat_map(|(k, v)| [k, v]))
    {
        if s.len() > 4096 || s.contains('\0') {
            return Err(PreparationError::Shape);
        }
    }
    paths(v)?;
    if !v.nonce.bytes().all(|b| b.is_ascii_alphanumeric())
        || v.vendor_version != VENDOR_VERSION
        || v.vendor_binary.physical_path != VENDOR_PATH
        || v.vendor_binary.logical_path != VENDOR_PATH
        || !crate::fixture::canonical_path(&v.logical_hint)
        || v.vendor_binary.metadata.length != VENDOR_LENGTH
        || v.vendor_binary.digest.as_str() != VENDOR_DIGEST
        || v.policy.assignment != hf_domain::owner_policy::FixtureAssignment::FixedEffect
        || v.source.generation != v.policy.generation
        || v.ledger.inode == 0
        || v.cwd != v.topology.root.path
    {
        return Err(PreparationError::Binding);
    }
    for p in [&v.gate_binary, &v.vendor_binary] {
        if p.metadata.length == 0
            || p.metadata.length > VENDOR_READ_LIMIT
            || p.metadata.identity.inode == 0
            || !(0..1_000_000_000).contains(&p.metadata.modified_nanoseconds)
            || !(0..1_000_000_000).contains(&p.metadata.changed_nanoseconds)
        {
            return Err(PreparationError::Binding);
        }
        for s in [&p.logical_path, &p.physical_path] {
            if !crate::fixture::canonical_path(s) {
                return Err(PreparationError::Shape);
            }
        }
    }
    if v.gate_binary.metadata.length > 128 * 1024 * 1024 {
        return Err(PreparationError::Binding);
    }
    for d in std::iter::once(&v.topology.root).chain(&v.topology.roles) {
        if !crate::fixture::canonical_path(&d.path) || d.identity.inode == 0 {
            return Err(PreparationError::Shape);
        }
    }
    for id in [
        &v.source.root,
        &v.source.database,
        &v.source.marker,
        &v.source.store_lock,
        &v.source.io_lock,
    ] {
        if id.inode == 0 {
            return Err(PreparationError::Binding);
        }
    }
    Ok(())
}

fn paths(v: &ClaudeQualification) -> Result<(), PreparationError> {
    for s in [
        &v.cwd,
        &v.logical_hint,
        &v.command.executable,
        &v.gate_binary.logical_path,
        &v.gate_binary.physical_path,
        &v.vendor_binary.logical_path,
        &v.vendor_binary.physical_path,
    ] {
        if !crate::fixture::canonical_path(s) {
            return Err(PreparationError::Shape);
        }
    }
    for d in std::iter::once(&v.topology.root).chain(&v.topology.roles) {
        if !crate::fixture::canonical_path(&d.path) {
            return Err(PreparationError::Shape);
        }
    }
    Ok(())
}
