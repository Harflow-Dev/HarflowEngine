#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Pure caller policy and the single task classifier, tested by hand-computed tables.
use hf_domain::{
    ctl::{
        caller::{
            CallerClass, EnvMarkers, GatePath, IdentityVerdict, PidObservation, RefusalKind, admit,
            agent_refused, ancestors_of, classify, is_candidate, marker_refusal, verdict,
        },
        task_projection::{is_unobserved, project},
    },
    process::ProcessIdentity,
};

const BOOT: &str = "abcdef01-2345-6789-abcd-ef0123456789";

fn identity(pid: u32, seconds: u64) -> ProcessIdentity {
    ProcessIdentity::new(pid, seconds, 1, BOOT.to_string()).unwrap()
}

#[test]
fn ancestors_exclude_self_and_stop_at_parent_zero_or_one() {
    let table = [(10, 9), (9, 8), (8, 1), (1, 0)];
    assert_eq!(ancestors_of(10, &table), Some(vec![9, 8]));
    assert_eq!(ancestors_of(9, &[(9, 0)]), Some(vec![]));
    assert_eq!(ancestors_of(9, &[(9, 1)]), Some(vec![]));
}

#[test]
fn ancestors_stop_at_a_missing_entry_and_at_a_cycle() {
    assert_eq!(ancestors_of(10, &[(10, 9), (9, 7)]), Some(vec![9, 7]));
    assert_eq!(
        ancestors_of(10, &[(10, 9), (9, 8), (8, 9)]),
        Some(vec![9, 8])
    );
    assert_eq!(ancestors_of(10, &[(10, 10)]), Some(vec![]));
    assert_eq!(ancestors_of(10, &[(10, 9), (9, 10)]), Some(vec![9]));
}

#[test]
fn a_process_missing_from_the_table_has_no_chain() {
    assert_eq!(ancestors_of(10, &[(9, 8)]), None);
    assert_eq!(ancestors_of(10, &[]), None);
}

#[test]
fn verdict_truth_table() {
    let recorded = identity(40, 100);
    let cases = [
        (
            Some(&recorded),
            PidObservation::Present(identity(40, 100)),
            IdentityVerdict::Alive,
        ),
        (
            Some(&recorded),
            PidObservation::Present(identity(40, 101)),
            IdentityVerdict::Dead,
        ),
        (
            Some(&recorded),
            PidObservation::Absent,
            IdentityVerdict::Dead,
        ),
        (
            Some(&recorded),
            PidObservation::Unknown,
            IdentityVerdict::Undecidable,
        ),
        (
            Some(&recorded),
            PidObservation::Zombie,
            IdentityVerdict::Undecidable,
        ),
        (None, PidObservation::Absent, IdentityVerdict::Undecidable),
        (
            None,
            PidObservation::Present(identity(40, 100)),
            IdentityVerdict::Undecidable,
        ),
    ];
    for (record, seen, wanted) in cases {
        assert_eq!(verdict(record, &seen), wanted, "{seen:?}");
    }
}

#[test]
fn classification_prefers_alive_then_undecidable() {
    use IdentityVerdict::{Alive, Dead, Undecidable};
    assert_eq!(classify(&[]), CallerClass::NoAgentDetected);
    assert_eq!(classify(&[Dead, Dead]), CallerClass::NoAgentDetected);
    assert_eq!(classify(&[Dead, Undecidable]), CallerClass::Undecidable);
    assert_eq!(classify(&[Undecidable, Alive, Dead]), CallerClass::Agent);
    assert_eq!(classify(&[Alive, Undecidable]), CallerClass::Agent);
}

#[test]
fn candidates_exclude_only_observed_terminal_records() {
    for status in ["completed", "failed", "timed_out", "cancelled"] {
        assert!(!is_candidate(status, Some(0)), "{status}");
        assert!(is_candidate(status, None), "{status}");
    }
    assert!(is_candidate("running", Some(0)));
    assert!(is_candidate("running", None));
    assert!(is_candidate("unknown", Some(0)));
}

#[test]
fn marker_rule_per_path() {
    let none = EnvMarkers::default();
    let empty = EnvMarkers {
        present: true,
        non_empty: false,
    };
    let full = EnvMarkers {
        present: true,
        non_empty: true,
    };
    for path in [GatePath::Managed, GatePath::Page, GatePath::Successors] {
        assert_eq!(marker_refusal(path, none), None);
        assert!(marker_refusal(path, full).is_some());
    }
    assert_eq!(marker_refusal(GatePath::Managed, empty), None);
    assert_eq!(marker_refusal(GatePath::Page, empty), None);
    assert!(marker_refusal(GatePath::Successors, empty).is_some());
}

#[test]
fn env_markers_observe_presence_and_content() {
    let lookup = |set: &'static [(&'static str, bool)]| {
        move |key: &str| set.iter().find(|entry| entry.0 == key).map(|entry| entry.1)
    };
    assert_eq!(EnvMarkers::observe(&lookup(&[])), EnvMarkers::default());
    let empty = EnvMarkers::observe(&lookup(&[("PB_DEPTH", false)]));
    assert_eq!((empty.present, empty.non_empty), (true, false));
    let mixed = EnvMarkers::observe(&lookup(&[("PB_DEPTH", false), ("HARFLOW_TASK_ID", true)]));
    assert_eq!((mixed.present, mixed.non_empty), (true, true));
    let other = EnvMarkers::observe(&lookup(&[("HOME", true)]));
    assert!(!other.present);
}

/// status, exit code, verdict, wire status, identity state, needs reconciliation.
#[test]
fn projection_table_against_the_reference_store() {
    use IdentityVerdict::{Alive, Dead, Undecidable};
    let table = [
        ("completed", Some(0), Alive, "completed", "dead", false),
        ("failed", Some(1), Undecidable, "failed", "dead", false),
        ("running", Some(0), Alive, "running", "alive", false),
        ("running", None, Alive, "running", "alive", false),
        ("running", None, Dead, "unknown", "dead", true),
        ("running", None, Undecidable, "running", "uncertain", true),
        ("failed", None, Alive, "running", "alive", true),
        ("failed", None, Dead, "unknown", "dead", true),
        ("completed", None, Alive, "running", "alive", true),
        ("completed", None, Dead, "completed", "dead", false),
        ("timed_out", None, Dead, "timed_out", "dead", false),
        ("cancelled", None, Dead, "cancelled", "dead", false),
        ("cancelled", None, Undecidable, "running", "uncertain", true),
    ];
    for (status, exit, live, wire, state, needs) in table {
        let got = project(status, exit, live);
        assert_eq!(
            (
                got.status.as_str(),
                got.identity_state,
                got.needs_reconciliation
            ),
            (wire, state, needs),
            "{status} {exit:?} {live:?}"
        );
        assert_eq!(got.active, is_unobserved(status, exit) && state != "dead");
    }
}

#[test]
fn a_running_status_is_unobserved_even_with_an_exit_code() {
    assert!(is_unobserved("running", Some(0)));
    assert!(is_unobserved("completed", None));
    assert!(!is_unobserved("completed", Some(0)));
}

#[test]
fn the_agent_switch_applies_to_managed_and_page_reads_only() {
    for refuse in [true, false] {
        assert_eq!(agent_refused(GatePath::Managed, refuse), refuse);
        assert_eq!(agent_refused(GatePath::Page, refuse), refuse);
        assert!(
            agent_refused(GatePath::Successors, refuse),
            "direct-only refuses an agent either way"
        );
    }
}

#[test]
fn admission_by_path_never_admits_undecidable_and_always_refuses_a_successor_agent() {
    for path in [GatePath::Managed, GatePath::Page, GatePath::Successors] {
        assert_eq!(admit(path, CallerClass::NoAgentDetected), None);
        assert_eq!(
            admit(path, CallerClass::Undecidable),
            Some(RefusalKind::Undecidable)
        );
    }
    assert_eq!(
        admit(GatePath::Successors, CallerClass::Agent),
        Some(RefusalKind::Agent)
    );
    // The managed and page answers follow the switch's current value, whichever it is.
    let managed = admit(GatePath::Managed, CallerClass::Agent);
    assert_eq!(
        managed.is_some(),
        agent_refused(
            GatePath::Managed,
            hf_domain::ctl::caller::REFUSE_UNRESOLVED_AGENTS
        )
    );
    assert_eq!(admit(GatePath::Page, CallerClass::Agent), managed);
}
