use base64::{Engine as _, engine::general_purpose::STANDARD};
use hf_application::ctl::state_recover::{
    evidence::OwnerEvidence,
    journal::{JOURNAL_BOUND, JournalError, RecoveryJournal},
};
use hf_domain::ctl::recovery_proposal::{EntryBody, FileIdentity, InventoryEntry};
use std::{collections::BTreeMap, fmt::Write as _};

pub(super) struct Writer(pub(super) Vec<u8>);
impl Writer {
    pub(super) fn raw(&mut self, text: &str) -> Result<(), JournalError> {
        if text.len() > JOURNAL_BOUND.saturating_sub(self.0.len()) {
            return Err(JournalError::TooLarge);
        }
        self.0.extend_from_slice(text.as_bytes());
        Ok(())
    }
    pub(super) fn string(&mut self, text: &str) -> Result<(), JournalError> {
        self.raw("\"")?;
        for ch in text.chars() {
            match ch {
                '"' => self.raw("\\\"")?,
                '\\' => self.raw("\\\\")?,
                '\n' => self.raw("\\n")?,
                '\r' => self.raw("\\r")?,
                '\t' => self.raw("\\t")?,
                '\u{8}' => self.raw("\\b")?,
                '\u{c}' => self.raw("\\f")?,
                ' '..='~' => self.raw(ch.encode_utf8(&mut [0; 4]))?,
                _ => {
                    for unit in ch.encode_utf16(&mut [0; 2]) {
                        write!(self, "\\u{unit:04x}").map_err(|_| JournalError::TooLarge)?;
                    }
                }
            }
        }
        self.raw("\"")
    }
    fn identity(&mut self, value: FileIdentity) -> Result<(), JournalError> {
        write!(self, "[{},{}]", value.dev, value.ino).map_err(|_| JournalError::TooLarge)
    }
    fn optional_identity(&mut self, value: Option<FileIdentity>) -> Result<(), JournalError> {
        match value {
            Some(id) => self.identity(id),
            None => self.raw("null"),
        }
    }
}
impl std::fmt::Write for Writer {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.raw(text).map_err(|_| std::fmt::Error)
    }
}

/// Sorted compact ensure-ASCII JSON including its newline, never growing past 16 MiB.
pub fn encode_journal(j: &RecoveryJournal) -> Result<Vec<u8>, JournalError> {
    j.validate()?;
    let mut out = Writer(Vec::with_capacity(JOURNAL_BOUND));
    out.raw("{\"backup_identity\":")?;
    out.optional_identity(j.backup_identity)?;
    out.raw(",\"backup_path\":")?;
    out.string(&j.backup_path)?;
    out.raw(",\"created_at\":")?;
    out.string(&j.created_at)?;
    out.raw(",\"data_identity\":")?;
    out.optional_identity(j.data_identity)?;
    out.raw(",\"engine_version\":")?;
    out.string(&j.engine_version)?;
    out.raw(",\"format\":\"harflow.state-recovery\",\"inventory\":")?;
    inventory(&mut out, &j.inventory.entries)?;
    out.raw(",\"lock_identity\":")?;
    out.identity(j.lock_identity)?;
    out.raw(",\"operation_id\":")?;
    out.string(&j.operation_id)?;
    out.raw(",\"owner_evidence\":")?;
    evidence(&mut out, &j.owner_evidence)?;
    out.raw(",\"phase\":")?;
    out.string(j.phase.as_str())?;
    out.raw(",\"previous_journal_bytes\":")?;
    previous(&mut out, j.previous_journal_bytes.as_deref())?;
    out.raw(",\"product\":\"harflow\",\"proposal\":")?;
    proposal(&mut out, &j.proposal)?;
    if let Some(result) = &j.result {
        out.raw(",\"result\":")?;
        super::result::write(&mut out, result)?;
    }
    out.raw(",\"root_identity\":")?;
    out.identity(j.root_identity)?;
    if let Some(files) = &j.staged {
        out.raw(",\"staged\":{")?;
        for (index, (name, file)) in files.iter().enumerate() {
            if index > 0 {
                out.raw(",")?;
            }
            out.string(name)?;
            out.raw(":{\"identity\":")?;
            out.identity(file.identity)?;
            write!(out, ",\"mtime_ns\":{},\"sha256\":", file.mtime_ns)
                .map_err(|_| JournalError::TooLarge)?;
            out.string(&file.sha256)?;
            write!(out, ",\"size\":{}}}", file.size).map_err(|_| JournalError::TooLarge)?;
        }
        out.raw("}")?;
    }
    if let Some(staging) = j.staging {
        out.raw(",\"staging\":")?;
        out.identity(staging)?;
    }
    out.raw(",\"state_root\":")?;
    out.string(&j.state_root)?;
    out.raw(",\"v\":1}\n")?;
    Ok(out.0)
}

fn evidence(out: &mut Writer, evidence: &OwnerEvidence) -> Result<(), JournalError> {
    out.raw("{\"basis\":")?;
    out.string(evidence.basis.as_str())?;
    out.raw(",\"checked\":[")?;
    for (index, id) in evidence.checked.iter().enumerate() {
        if index > 0 {
            out.raw(",")?;
        }
        out.raw("{\"boot_uuid\":")?;
        out.string(id.boot_uuid())?;
        write!(
            out,
            ",\"pid\":{},\"start_microseconds\":{},\"start_seconds\":{}}}",
            id.pid(),
            id.start_microseconds(),
            id.start_seconds()
        )
        .map_err(|_| JournalError::TooLarge)?;
    }
    out.raw("]}")
}

fn previous(out: &mut Writer, bytes: Option<&[u8]>) -> Result<(), JournalError> {
    let Some(bytes) = bytes else {
        return out.raw("null");
    };
    out.raw("\"")?;
    for chunk in bytes.chunks(3) {
        let mut buffer = [0u8; 4];
        let len = STANDARD
            .encode_slice(chunk, &mut buffer)
            .map_err(|_| JournalError::Invalid)?;
        out.raw(std::str::from_utf8(&buffer[..len]).map_err(|_| JournalError::Invalid)?)?;
    }
    out.raw("\"")
}

fn proposal(
    out: &mut Writer,
    p: &hf_domain::ctl::recovery_proposal::RecoveryProposal,
) -> Result<(), JournalError> {
    out.raw("{\"engine_version\":")?;
    out.string(&p.engine_version)?;
    out.raw(",\"inventory_sha256\":")?;
    out.string(&p.inventory_sha256)?;
    out.raw(",\"lock_identity\":")?;
    out.optional_identity(p.lock_identity)?;
    out.raw(",\"operation_id\":")?;
    out.string(&p.operation_id)?;
    out.raw(",\"prior_journal_sha256\":")?;
    match &p.prior_journal_sha256 {
        Some(v) => out.string(v)?,
        None => out.raw("null")?,
    }
    out.raw(",\"root_identity\":")?;
    out.identity(p.root_identity)?;
    out.raw(",\"state_root\":")?;
    out.string(&p.state_root)?;
    out.raw("}")
}

enum Part<'a> {
    Children(&'a BTreeMap<String, InventoryEntry>, usize),
    Entry(&'a InventoryEntry, usize),
    Name(&'a str),
    Raw(&'static str),
}
fn inventory(
    out: &mut Writer,
    entries: &BTreeMap<String, InventoryEntry>,
) -> Result<(), JournalError> {
    use hf_application::ctl::state_recover::journal::INVENTORY_DEPTH_BOUND;
    use hf_domain::ctl::recovery_proposal::INVENTORY_BOUND;
    let mut stack = vec![Part::Children(entries, 0)];
    let mut count = 0;
    while let Some(part) = stack.pop() {
        match part {
            Part::Raw(text) => out.raw(text)?,
            Part::Name(name) => {
                out.string(name)?;
                out.raw(":")?;
            }
            Part::Children(children, depth) => {
                if depth > INVENTORY_DEPTH_BOUND {
                    return Err(JournalError::TooLarge);
                }
                out.raw("{")?;
                stack.push(Part::Raw("}"));
                for (index, (name, entry)) in children.iter().enumerate().rev() {
                    count += 1;
                    if count > INVENTORY_BOUND {
                        return Err(JournalError::TooLarge);
                    }
                    stack.push(Part::Entry(entry, depth));
                    stack.push(Part::Name(name));
                    if index > 0 {
                        stack.push(Part::Raw(","));
                    }
                }
            }
            Part::Entry(entry, depth) => {
                let s = entry.stat;
                write!(
                    out,
                    "[{},{},{},{},{}",
                    s.dev, s.ino, s.mode, s.size, s.mtime_ns
                )
                .map_err(|_| JournalError::TooLarge)?;
                stack.push(Part::Raw("]"));
                match &entry.body {
                    EntryBody::File => {}
                    EntryBody::Link(target) => {
                        out.raw(",")?;
                        out.string(target)?;
                    }
                    EntryBody::Directory(children) => {
                        out.raw(",")?;
                        stack.push(Part::Children(children, depth + 1));
                    }
                }
            }
        }
    }
    Ok(())
}
