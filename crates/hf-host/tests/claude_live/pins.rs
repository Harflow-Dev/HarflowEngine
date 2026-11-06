use std::{fs, io::Read, os::unix::fs::MetadataExt, path::Path, time::Instant};
const VENDOR: &str = "/Users/nayanda/.local/share/claude/versions/2.1.318";
const HASH: &str = "677a2ea7bf576cedd20e191d973914ed8a41796f53a63b1923381ddc4d5b5e7d";
const ICU: &str = "/usr/share/icu/icudt78l.dat";
pub(super) struct Pins {
    vendor: hf_domain::prepared_launch::BinaryPin,
    icu: fs::Metadata,
}

pub(super) fn observe(until: Instant) -> Pins {
    let vendor = hf_platform::prepared_launch::observe_qualification_vendor(
        Path::new(VENDOR),
        &hf_storage::Sha256Digest,
        until,
    )
    .expect("physical vendor pin");
    assert_eq!(vendor.logical_path, VENDOR);
    assert_eq!(vendor.physical_path, VENDOR);
    assert_eq!(vendor.metadata.length, 319_598_464);
    assert_eq!(vendor.digest.as_str(), HASH);
    let before = fs::symlink_metadata(ICU).expect("ICU identity");
    assert!(
        before.is_file()
            && !before.file_type().is_symlink()
            && before.nlink() == 1
            && before.len() == 35_444_880
    );
    assert_eq!(fs::canonicalize(ICU).unwrap(), Path::new(ICU));
    let mut file = fs::File::open(ICU).unwrap();
    let mut hash = hf_storage::Sha256Stream::with_limit(128 * 1024 * 1024).unwrap();
    let mut buf = [0; 16384];
    loop {
        assert!(Instant::now() < until, "ICU observation deadline");
        let n = file.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]).unwrap();
    }
    assert_eq!(
        hash.finish().unwrap().as_str(),
        "666a97d4dd1139f537290d74a661cc6b6da6cc58da24486b3cb282eacbcfa549"
    );
    let icu = fs::symlink_metadata(ICU).unwrap();
    assert!(super::identity::same_metadata(&before, &icu));
    Pins { vendor, icu }
}
pub(super) fn unchanged(before: &Pins, until: Instant) {
    let after = observe(until);
    assert_eq!(before.vendor, after.vendor, "vendor bookend");
    assert!(
        super::identity::same_metadata(&before.icu, &after.icu),
        "ICU bookend"
    );
}
