use super::tree::Tree;
use hf_application::ctl::state_recover::journal::{INVENTORY_DEPTH_BOUND, JournalError};
use hf_domain::ctl::recovery_proposal::{
    EntryBody, EntryStat, INVENTORY_BOUND, INVENTORY_EXCLUDED, InventoryEntry,
};
use std::collections::BTreeMap;

struct Frame {
    remaining: std::vec::IntoIter<(String, usize)>,
    entries: BTreeMap<String, InventoryEntry>,
    parent: Option<(String, EntryStat)>,
}
fn frame(t: &Tree, id: usize, parent: Option<(String, EntryStat)>) -> Result<Frame, JournalError> {
    Ok(Frame {
        remaining: t
            .object(id)?
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect::<Vec<_>>()
            .into_iter(),
        entries: BTreeMap::new(),
        parent,
    })
}
pub fn decode(t: &Tree, id: usize) -> Result<BTreeMap<String, InventoryEntry>, JournalError> {
    let mut stack = vec![frame(t, id, None)?];
    let mut count = 0usize;
    loop {
        let depth = stack.len() - 1;
        let next = stack.last_mut().and_then(|f| f.remaining.next());
        let Some((name, id)) = next else {
            let done = stack.pop().ok_or(JournalError::Invalid)?;
            let Some((name, stat)) = done.parent else {
                return Ok(done.entries);
            };
            let parent = stack.last_mut().ok_or(JournalError::Invalid)?;
            parent.entries.insert(
                name,
                InventoryEntry {
                    stat,
                    body: EntryBody::Directory(done.entries),
                },
            );
            continue;
        };
        count += 1;
        if count > INVENTORY_BOUND {
            return Err(JournalError::TooLarge);
        }
        if name.is_empty()
            || matches!(name.as_str(), "." | "..")
            || name.contains('/')
            || name.contains('\0')
            || (depth == 0 && INVENTORY_EXCLUDED.contains(&name.as_str()))
        {
            return Err(JournalError::Invalid);
        }
        let a = t.array(id)?;
        if !(5..=6).contains(&a.len()) {
            return Err(JournalError::Invalid);
        }
        let stat = EntryStat {
            dev: t.signed(a[0])?,
            ino: t.unsigned(a[1])?,
            mode: u32::try_from(t.unsigned(a[2])?).map_err(|_| JournalError::Invalid)?,
            size: t.signed(a[3])?,
            mtime_ns: t.signed(a[4])?,
        };
        if stat.size < 0 {
            return Err(JournalError::Invalid);
        }
        let body = match (stat.mode & 0o170000, a.len()) {
            (0o100000, 5) => EntryBody::File,
            (0o120000, 6) => {
                let target = t.text(a[5])?;
                if target.contains('\0') {
                    return Err(JournalError::Invalid);
                }
                EntryBody::Link(target)
            }
            (0o040000, 6) => {
                if depth >= INVENTORY_DEPTH_BOUND {
                    return Err(JournalError::TooLarge);
                }
                stack.push(frame(t, a[5], Some((name, stat)))?);
                continue;
            }
            _ => return Err(JournalError::Invalid),
        };
        stack
            .last_mut()
            .ok_or(JournalError::Invalid)?
            .entries
            .insert(name, InventoryEntry { stat, body });
    }
}
