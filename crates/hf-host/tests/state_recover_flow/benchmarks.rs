use super::{support::*, *};
use hf_application::ctl::state_inspect::InventoryPort;
use hf_domain::ctl::recovery_proposal::{EntryBody, InventoryEntry};
use std::collections::BTreeMap;
#[test]
#[ignore = "explicit B4 sparse 2GiB archive measurement"]
fn sparse_two_gib_archive_benchmark() {
    let _serial = serial();
    let f = Fixture::new();
    f.seed();
    let path = f.0.join("large-blob");
    let file = fs::File::create(&path).unwrap();
    file.set_len(2 * 1024 * 1024 * 1024).unwrap();
    file.sync_all().unwrap();
    let metadata = file.metadata().unwrap();
    let id = file_identity(&path);
    drop(file);
    let start = Instant::now();
    let p = f.proposal();
    let s = f.session(&p);
    let result = f.run(&p, &s, &NoArchiveFault, &s).unwrap();
    assert_eq!(result.journal.phase, JournalPhase::BackedUp);
    let archived = PathBuf::from(p.backup_path()).join("data/large-blob");
    assert_eq!(file_identity(&archived), id);
    assert_eq!(fs::metadata(&archived).unwrap().len(), metadata.len());
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(archived).unwrap();
    for offset in [0, 1024 * 1024 * 1024, metadata.len() - 4096] {
        file.seek(SeekFrom::Start(offset)).unwrap();
        let mut bytes = [1; 4096];
        file.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [0; 4096]);
    }
    eprintln!(
        "B4 sparse logical={} allocated={} elapsed={:?}",
        metadata.len(),
        metadata.blocks() * 512,
        start.elapsed()
    );
}
fn count(entries: &BTreeMap<String, InventoryEntry>) -> usize {
    entries
        .values()
        .map(|entry| {
            1 + match &entry.body {
                EntryBody::Directory(children) => count(children),
                _ => 0,
            }
        })
        .sum()
}
#[test]
#[ignore = "explicit B4 exactly 100000 inventoried entries archive measurement"]
fn hundred_thousand_total_entries_benchmark() {
    let _serial = serial();
    let f = Fixture::new();
    f.seed();
    let root = f.root();
    let initial = FsInventory.walk(&root, 100000).unwrap();
    let existing = count(&initial.entries);
    let payload = f.0.join("bulk");
    fs::create_dir(&payload).unwrap();
    for n in 0..(100000 - existing - 1) {
        fs::File::create(payload.join(format!("{n:x}"))).unwrap();
    }
    let start = Instant::now();
    let inventory = FsInventory.walk(&root, 100000).unwrap();
    assert_eq!(count(&inventory.entries), 100000);
    let p = f.proposal();
    let s = f.session(&p);
    let result = f.run(&p, &s, &NoArchiveFault, &s).unwrap();
    assert_eq!(result.journal.phase, JournalPhase::BackedUp);
    let bytes = fs::metadata(f.0.join(".harflow-recovery.json"))
        .unwrap()
        .len();
    assert!(bytes <= 16 * 1024 * 1024);
    assert_eq!(
        fs::read_dir(PathBuf::from(p.backup_path()).join("data/bulk"))
            .unwrap()
            .count(),
        100000 - existing - 1
    );
    eprintln!(
        "B4 entries=100000 journal_bytes={bytes} elapsed={:?}",
        start.elapsed()
    );
}
