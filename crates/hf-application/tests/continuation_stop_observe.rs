#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The domain projection (hf-domain permits no test target of its own, so it is exercised here)
//! of a task's stored chain root and its chain's stop row.
use hf_domain::{
    ctl::{
        continuation_stop::{ContinuationStop, StopRow, StopState},
        task_rows::ChainRoot,
    },
    task_read::TaskId,
};

const OWN: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ROOT: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const CANCELLED: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

fn id(value: &str) -> TaskId {
    TaskId::new(value.to_owned()).unwrap()
}

fn marker(text: Option<&str>) -> StopRow {
    StopRow::Marker {
        attribution: text.map(str::to_owned),
    }
}

fn observe(root: Option<&str>, row: &StopRow) -> ContinuationStop {
    ContinuationStop::observe(&id(OWN), &ChainRoot::qualify(root.map(str::to_owned)), row)
}

#[test]
fn states_render_their_wire_names() {
    assert_eq!(StopState::Stopped.as_str(), "stopped");
    assert_eq!(StopState::NotStopped.as_str(), "not_stopped");
    assert_eq!(StopState::Unknown.as_str(), "unknown");
}

#[test]
fn an_absent_row_is_not_stopped_at_the_stored_root() {
    let stop = observe(Some(ROOT), &StopRow::Absent);
    assert_eq!(stop.state, StopState::NotStopped);
    assert_eq!(stop.root, Some(id(ROOT)));
    assert_eq!(stop.cancelled, None);
}

#[test]
fn an_absent_stored_root_makes_the_task_its_own_root() {
    for row in [StopRow::Absent, marker(None), StopRow::Fault] {
        assert_eq!(observe(None, &row).root, Some(id(OWN)), "{row:?}");
    }
}

#[test]
fn a_marker_is_stopped_and_keeps_a_valid_attribution() {
    let stop = observe(Some(ROOT), &marker(Some(CANCELLED)));
    assert_eq!(stop.state, StopState::Stopped);
    assert_eq!(stop.root, Some(id(ROOT)));
    assert_eq!(stop.cancelled, Some(id(CANCELLED)));
}

#[test]
fn a_marker_without_or_with_unusable_attribution_has_no_cancelled_task() {
    let long = "a".repeat(65);
    for text in [None, Some(""), Some("not a valid id!"), Some(long.as_str())] {
        let stop = observe(Some(ROOT), &marker(text));
        assert_eq!(stop.state, StopState::Stopped, "{text:?}");
        assert_eq!(stop.root, Some(id(ROOT)), "{text:?}");
        assert_eq!(stop.cancelled, None, "{text:?}");
    }
}

#[test]
fn a_fault_is_unknown_but_keeps_the_root() {
    let stop = observe(Some(ROOT), &StopRow::Fault);
    assert_eq!(stop.state, StopState::Unknown);
    assert_eq!(stop.root, Some(id(ROOT)));
    assert_eq!(stop.cancelled, None);
    assert_eq!(observe(None, &StopRow::Fault).root, Some(id(OWN)));
}

#[test]
fn an_invalid_stored_root_is_unknown_with_no_root_whatever_the_row_says() {
    let long = "a".repeat(70);
    for root in ["not a valid id!", "", long.as_str()] {
        for row in [
            StopRow::Absent,
            marker(Some(CANCELLED)),
            marker(None),
            StopRow::Fault,
        ] {
            let stop = observe(Some(root), &row);
            assert_eq!(stop.state, StopState::Unknown, "{root:?} {row:?}");
            assert_eq!(stop.root, None, "{root:?} {row:?}");
            assert_eq!(stop.cancelled, None, "{root:?} {row:?}");
        }
    }
}

#[test]
fn an_invalid_root_never_falls_back_to_the_task_id() {
    let stop = observe(Some(""), &StopRow::Absent);
    assert_ne!(stop.root, Some(id(OWN)));
    assert_eq!(stop.root, None);
}
