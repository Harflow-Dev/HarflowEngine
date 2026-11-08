#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The statistics-page use case over scripted ports.
mod statistics_support;
mod task_reads_support;

#[allow(unused_imports)]
use hf_application::ctl::{state_inspect::RootClass, task_reads::*};
#[allow(unused_imports)]
use hf_domain::ctl::{caller::*, statistics::*, statistics_time::*};
#[allow(unused_imports)]
use statistics_support::*;
#[allow(unused_imports)]
use std::time::Duration;
#[allow(unused_imports)]
use task_reads_support::*;

#[test]
fn paging_stops_at_the_limit_and_names_the_next_offset() {
    let world = world((0..5).map(|n| done(&format!("t{n}"), n)).collect());
    let page = ready(run(
        &world,
        &request(2, StatsCursorArg::Absent),
        &Sizes { cursor: 7, ..SMALL },
    ));
    assert_eq!(
        (page.items.len(), page.has_more, page.next_cursor.as_deref()),
        (2, true, Some("ccccccc"))
    );
    let page = ready(run(
        &world,
        &request(2, bound(Some(&"g".repeat(64)), 4)),
        &SMALL,
    ));
    assert_eq!(
        (page.items.len(), page.has_more, page.next_cursor),
        (1, false, None)
    );
}

#[test]
fn the_candidate_bound_is_258048_and_a_byte_over_stops_the_page() {
    let world = world((0..4).map(|n| done(&format!("t{n}"), n)).collect());
    for (edge, count) in [(258_048, 3), (258_049, 2)] {
        let sizes = Sizes {
            candidate: if edge == 258_048 {
                |n| if n == 3 { 258_048 } else { 10 }
            } else {
                |n| if n == 3 { 258_049 } else { 10 }
            },
            ..SMALL
        };
        let page = ready(run(&world, &request(3, StatsCursorArg::Absent), &sizes));
        assert_eq!(page.items.len(), count, "edge {edge}");
    }
}

#[test]
fn a_first_item_over_the_page_is_page_limit() {
    let world = world(vec![done("a", 0)]);
    let sizes = Sizes {
        candidate: |_| 258_049,
        ..SMALL
    };
    assert_eq!(
        unavailable(run(&world, &request(1, StatsCursorArg::Absent), &sizes)),
        diag("page_limit", "One statistics fact exceeds the bounded page")
    );
}

#[test]
fn the_cursor_and_the_final_document_have_their_own_bounds() {
    let world = world(vec![done("a", 0), done("b", 1)]);
    let long = Sizes {
        cursor: 4097,
        ..SMALL
    };
    assert_eq!(
        unavailable(run(&world, &request(1, StatsCursorArg::Absent), &long)),
        diag("page_limit", "Statistics cursor byte budget exceeded")
    );
    let exact = Sizes {
        cursor: 4096,
        document: 262_144,
        ..SMALL
    };
    assert!(ready(run(&world, &request(1, StatsCursorArg::Absent), &exact)).has_more);
    let over = Sizes {
        document: 262_145,
        ..SMALL
    };
    assert_eq!(
        unavailable(run(&world, &request(1, StatsCursorArg::Absent), &over)),
        diag("page_limit", "Statistics page byte budget exceeded")
    );
}
