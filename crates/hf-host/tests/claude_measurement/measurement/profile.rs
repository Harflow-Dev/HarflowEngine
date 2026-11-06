use hf_application::owner_runtime::DigestPort;

use super::MeasurementMode;

const BANNER_RULE: &str = "(allow sysctl-read (sysctl-name \"kern.osproductversion\"))\n";
const PAGE_SIZE_RULE: &str = "(allow sysctl-read (sysctl-name \"hw.pagesize\"))\n";
const COMPAT_PAGE_SIZE_RULE: &str = "(allow sysctl-read (sysctl-name \"hw.pagesize_compat\"))\n";

pub(super) fn select(mode: MeasurementMode, base: &str) -> String {
    let addition = match mode {
        MeasurementMode::FullFixture => "",
        MeasurementMode::VersionOnlySysctl => BANNER_RULE,
        MeasurementMode::PageSizeVersionOnly => PAGE_SIZE_RULE,
        MeasurementMode::CompatPageSizeVersionOnly
        | MeasurementMode::CompatProtocol
        | MeasurementMode::OwnerQualification => COMPAT_PAGE_SIZE_RULE,
    };
    assert!(
        !base.contains("sysctl-read"),
        "unchanged base lacks sysctl exceptions"
    );
    let candidate = format!("{base}{addition}");
    assert!(candidate.starts_with(base));
    assert_eq!(&candidate[base.len()..], addition);
    candidate
}

pub(super) fn delta(mode: MeasurementMode) -> &'static str {
    match mode {
        MeasurementMode::FullFixture => "none",
        MeasurementMode::VersionOnlySysctl => BANNER_RULE.trim(),
        MeasurementMode::PageSizeVersionOnly => PAGE_SIZE_RULE.trim(),
        MeasurementMode::CompatPageSizeVersionOnly
        | MeasurementMode::CompatProtocol
        | MeasurementMode::OwnerQualification => COMPAT_PAGE_SIZE_RULE.trim(),
    }
}

pub(super) fn hash(profile: &str) -> String {
    hf_storage::Sha256Digest
        .digest_parts(&[profile.as_bytes()])
        .as_str()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagesize_candidate_is_exactly_one_appended_rule() {
        let base = "(version 1)\n";
        let candidate = select(MeasurementMode::PageSizeVersionOnly, base);
        assert_eq!(candidate, format!("{base}{PAGE_SIZE_RULE}"));
        assert_eq!(
            delta(MeasurementMode::PageSizeVersionOnly),
            PAGE_SIZE_RULE.trim()
        );
        assert!(!candidate.contains("kern.osproductversion"));
    }

    #[test]
    fn existing_modes_retain_their_exact_profiles() {
        let base = "(version 1)\n";
        assert_eq!(select(MeasurementMode::FullFixture, base), base);
        assert_eq!(
            select(MeasurementMode::VersionOnlySysctl, base),
            format!("{base}{BANNER_RULE}")
        );
    }

    #[test]
    fn compat_pagesize_candidate_is_exactly_the_single_authorized_rule() {
        let base = "(version 1)\n";
        let candidate = select(MeasurementMode::CompatPageSizeVersionOnly, base);
        assert_eq!(candidate, format!("{base}{COMPAT_PAGE_SIZE_RULE}"));
        assert_eq!(
            &candidate[base.len()..],
            "(allow sysctl-read (sysctl-name \"hw.pagesize_compat\"))\n"
        );
        assert!(!candidate.contains("hw.pagesize\""));
        assert!(!candidate.contains("kern.osproductversion"));
    }

    #[test]
    fn compat_candidate_preserves_all_older_profile_modes() {
        let base = "(version 1)\n";
        assert_eq!(select(MeasurementMode::FullFixture, base), base);
        assert_eq!(
            select(MeasurementMode::VersionOnlySysctl, base),
            format!("{base}{BANNER_RULE}")
        );
        assert_eq!(
            select(MeasurementMode::PageSizeVersionOnly, base),
            format!("{base}{PAGE_SIZE_RULE}")
        );
    }
}
