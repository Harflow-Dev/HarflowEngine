#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "caller_ancestry/support/mod.rs"]
mod support;

use hf_platform::task_runtime::caller::observe_caller_with_source;
use std::time::{Duration, Instant};
use support::{Record, Script, chain, deadline, observations, repeated_boots, repeated_records};

fn run(
    records: Vec<Record>,
    boots: Vec<String>,
) -> (
    std::io::Result<hf_platform::task_runtime::caller::CallerObservation>,
    Script,
) {
    let init = Record {
        pid: 1,
        parent: 0,
        uid: 0,
        status: 2,
        seconds: 1,
    };
    let mut evidence = Script::new(repeated_records(&records, init), boots);
    let result = observe_caller_with_source(
        records.first().map_or(1, |r| r.pid),
        deadline(),
        &mut evidence,
    );
    (result, evidence)
}

#[test]
fn accepts_complete_root_owned_chain_and_mixed_case_constant_boot_uuid() {
    let records = chain(3);
    let (result, evidence) = run(records, repeated_boots(16));
    let observation = result.expect("complete chain");
    assert_eq!(observation.chain.len(), 3);
    assert_eq!(observation.chain[0].uid, 392);
    assert!(observation.chain[1..].iter().all(|item| item.uid == 0));
    assert_eq!(observation.init.uid, 0);
    assert_eq!(
        evidence.reads.iter().filter(|read| *read == "boot").count(),
        16
    );
}

#[test]
fn actual_caller_uid_must_match_real_uid_but_ancestors_may_be_root() {
    let mut records = chain(2);
    records[0].uid = 396;
    let (result, _) = run(records, repeated_boots(12));
    assert!(result.is_err());
}

#[test]
fn ancestor_uid_may_differ_from_the_actual_callers_uid_without_being_root() {
    let mut records = chain(2);
    records[1].uid = 777;
    let (result, _) = run(records, repeated_boots(12));
    assert!(result.is_ok());
}

#[test]
fn exactly_128_links_succeed_with_bounded_provider_calls() {
    let records = chain(128);
    let (result, evidence) = run(records, repeated_boots(516));
    assert_eq!(result.expect("128 links").chain.len(), 128);
    assert_eq!(
        evidence
            .reads
            .iter()
            .filter(|read| read.starts_with("record:"))
            .count(),
        258
    );
    assert_eq!(
        evidence.reads.iter().filter(|read| *read == "boot").count(),
        516
    );
}

#[test]
fn refuses_129_links_cycles_and_incomplete_parent_edges() {
    let records = chain(129);
    assert!(run(records, repeated_boots(516)).0.is_err());

    let mut cycle = chain(3);
    cycle[2].parent = cycle[0].pid;
    assert!(run(cycle, repeated_boots(16)).0.is_err());

    for parent in [0, 2] {
        let mut broken = chain(1);
        broken[0].parent = parent;
        assert!(run(broken, repeated_boots(8)).0.is_err());
    }
}

#[test]
fn refuses_missing_or_malformed_init_anchor_and_read_errors_stop() {
    let mut missing_init = chain(1);
    missing_init[0].parent = 0;
    assert!(run(missing_init, repeated_boots(8)).0.is_err());

    for change in 0..4 {
        let mut records = observations(
            &chain(1),
            Record {
                pid: 1,
                parent: 0,
                uid: 0,
                status: 2,
                seconds: 1,
            },
        );
        match change {
            0 => records[0].parent = 7,
            1 => records[0].uid = 1,
            2 => records[0].status = 0,
            _ => records[0].start_seconds = 0,
        }
        let mut evidence = Script::new(records, repeated_boots(8));
        assert!(observe_caller_with_source(1000, deadline(), &mut evidence).is_err());
    }

    let mut evidence = Script::new(
        observations(
            &chain(1),
            Record {
                pid: 1,
                parent: 0,
                uid: 0,
                status: 2,
                seconds: 1,
            },
        ),
        repeated_boots(8),
    );
    evidence.fail_record(0);
    assert!(observe_caller_with_source(1000, deadline(), &mut evidence).is_err());
    assert_eq!(
        evidence.reads,
        vec!["boot".to_owned(), "record:1".to_owned()]
    );
}

#[test]
fn reobservation_detects_identity_uid_parent_and_boot_changes() {
    let base = chain(2);
    let init = Record {
        pid: 1,
        parent: 0,
        uid: 0,
        status: 2,
        seconds: 1,
    };
    let baseline = repeated_records(&base, init);
    let final_init = baseline.len() - 1;
    let mutations = [
        ("caller start seconds", 3, 0),
        ("caller UID", 3, 1),
        ("caller parent", 3, 2),
        ("caller PID", 3, 3),
        ("caller microseconds", 3, 4),
        ("ancestor UID", 4, 5),
        ("ancestor parent", 4, 6),
        ("ancestor unsupported status", 4, 7),
        ("init UID", final_init, 8),
        ("init parent", final_init, 9),
        ("init unsupported status", final_init, 10),
        ("init start seconds", final_init, 11),
        ("init microseconds", final_init, 12),
    ];
    for (label, index, mutation) in mutations {
        let mut scripted = baseline.clone();
        match mutation {
            0 => scripted[index].start_seconds += 1,
            1 => scripted[index].uid = 7,
            2 => scripted[index].parent = 1,
            3 => scripted[index].pid = 9999,
            4 => scripted[index].start_microseconds += 1,
            5 => scripted[index].uid = 777,
            6 => scripted[index].parent = 2,
            7 => scripted[index].status = 5,
            8 => scripted[index].uid = 1,
            9 => scripted[index].parent = 9,
            10 => scripted[index].status = 5,
            11 => scripted[index].start_seconds += 1,
            _ => scripted[index].start_microseconds += 1,
        }
        assert_ne!(scripted, baseline, "{label} fixture must change");
        let mut evidence = Script::new(scripted, repeated_boots(12));
        assert!(
            observe_caller_with_source(1000, deadline(), &mut evidence).is_err(),
            "re-observation must refuse {label} change"
        );
    }

    let mut boot_change = repeated_boots(12);
    boot_change[5] = "12345678-1234-1234-1234-123456789abc".to_owned();
    assert!(run(base.clone(), boot_change).0.is_err());

    let mut interlink_boot = repeated_boots(12);
    interlink_boot[4] = "12345678-1234-1234-1234-123456789abc".to_owned();
    interlink_boot[5] = "12345678-1234-1234-1234-123456789abc".to_owned();
    assert!(run(base, interlink_boot).0.is_err());
}

#[test]
fn raw_case_only_boot_change_inside_a_bookend_pair_refuses() {
    let mut boots = repeated_boots(8);
    boots[2] = "abcdef01-2345-6789-abcd-ef0123456789".to_owned();
    boots[3] = "ABCDEF01-2345-6789-ABCD-EF0123456789".to_owned();
    assert!(run(chain(1), boots).0.is_err());
}

#[test]
fn missing_or_failed_reobservation_and_final_init_reads_stop() {
    let records = chain(2);
    for failed_record in [3, 5] {
        for missing in [false, true] {
            let mut evidence = Script::new(
                repeated_records(
                    &records,
                    Record {
                        pid: 1,
                        parent: 0,
                        uid: 0,
                        status: 2,
                        seconds: 1,
                    },
                ),
                repeated_boots(12),
            );
            if missing {
                evidence.records.truncate(failed_record);
            } else {
                evidence.fail_record(failed_record);
            }
            assert!(observe_caller_with_source(1000, deadline(), &mut evidence).is_err());
            let pids = ["1", "1000", "1001", "1000", "1001", "1"];
            let mut expected_reads = Vec::new();
            for pid in &pids[..failed_record] {
                expected_reads.extend([
                    "boot".to_owned(),
                    format!("record:{pid}"),
                    "boot".to_owned(),
                ]);
            }
            expected_reads.extend(["boot".to_owned(), format!("record:{}", pids[failed_record])]);
            assert_eq!(
                evidence.reads, expected_reads,
                "failed record {failed_record}, missing={missing}"
            );
        }
    }
}

#[test]
fn valid_status_transition_between_observations_is_allowed() {
    let records = chain(1);
    let mut scripted = repeated_records(
        &records,
        Record {
            pid: 1,
            parent: 0,
            uid: 0,
            status: 2,
            seconds: 1,
        },
    );
    scripted[2].status = 3;
    let mut evidence = Script::new(scripted, repeated_boots(8));
    assert!(observe_caller_with_source(1000, deadline(), &mut evidence).is_ok());
}

#[test]
fn malformed_boot_uuid_refuses_observation() {
    let records = chain(1);
    let (result, _) = run(records, vec!["malformed".to_owned(); 8]);
    assert!(result.is_err());
}

#[test]
fn invalid_caller_pid_refuses_before_any_evidence_read() {
    for pid in [0, 1, u32::MAX] {
        let mut evidence = Script::new(Vec::new(), Vec::new());
        assert!(observe_caller_with_source(pid, deadline(), &mut evidence).is_err());
        assert!(evidence.reads.is_empty());
    }
}

#[test]
fn scripted_expiry_before_or_after_each_evidence_call_stops_later_reads() {
    let records = chain(1);
    let init = Record {
        pid: 1,
        parent: 0,
        uid: 0,
        status: 2,
        seconds: 1,
    };
    let total_calls = 12;
    let limit = Instant::now() + Duration::from_secs(60);
    let before = limit - Duration::from_secs(1);

    for call_index in 0..total_calls {
        for after_call in [false, true] {
            let mut evidence = Script::new(repeated_records(&records, init), repeated_boots(8));
            let mut times = vec![before; total_calls * 2];
            times[call_index * 2 + usize::from(after_call)] = limit;
            evidence.clock_at(times);
            assert!(observe_caller_with_source(1000, limit, &mut evidence).is_err());
            let expected_reads = call_index + usize::from(after_call);
            assert_eq!(
                evidence.reads.len(),
                expected_reads,
                "call {call_index}, after={after_call}"
            );
        }
    }
}

#[test]
fn malformed_status_and_boot_record_errors_stop_immediately() {
    let records = chain(1);
    let init = Record {
        pid: 1,
        parent: 0,
        uid: 0,
        status: 2,
        seconds: 1,
    };
    let mut scripted = repeated_records(&records, init);
    scripted[1].status = 5;
    let mut evidence = Script::new(scripted, repeated_boots(8));
    assert!(observe_caller_with_source(1000, deadline(), &mut evidence).is_err());
    assert_eq!(
        evidence.reads,
        vec![
            "boot".to_owned(),
            "record:1".to_owned(),
            "boot".to_owned(),
            "boot".to_owned(),
            "record:1000".to_owned()
        ]
    );

    let mut evidence = Script::new(repeated_records(&records, init), repeated_boots(8));
    evidence.fail_boot(0);
    assert!(observe_caller_with_source(1000, deadline(), &mut evidence).is_err());
    assert_eq!(evidence.reads, vec!["boot".to_owned()]);
}
