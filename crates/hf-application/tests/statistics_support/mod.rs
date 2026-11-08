#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The statistics-page use case over scripted ports: gate order, parameter and cursor order, the
//! scan phase and the projection phase, the generation, the paging bounds and a real expiry of
//! the statistics budget.

use crate::task_reads_support::{NONE_MARKERS, World, root, row, stat_facts};
use hf_application::ctl::task_reads::{
    ReadError, StatsCursorArg, StatsMeta, StatsOut, StatsRequest, StatsSizing, statistics,
};
use hf_domain::ctl::{
    statistics::{StatFacts, StatItem},
    statistics_time::{Bounds, bounds},
};

pub const FIRST: i64 = 1_768_039_200_000_000;

pub struct Sizes {
    pub candidate: fn(usize) -> usize,
    pub document: usize,
    pub cursor: usize,
    pub oversized: bool,
}

pub const SMALL: Sizes = Sizes {
    candidate: |_| 10,
    document: 10,
    cursor: 10,
    oversized: false,
};

impl StatsSizing for Sizes {
    fn source_oversized(&self, _: &StatFacts) -> bool {
        self.oversized
    }
    fn candidate(&self, _: &StatsMeta, items: &[StatItem]) -> usize {
        (self.candidate)(items.len())
    }
    fn cursor(&self, _: &str, _: usize) -> String {
        "c".repeat(self.cursor)
    }
    fn document(&self, _: &StatsMeta, _: &[StatItem], _: Option<&str>) -> usize {
        self.document
    }
}

pub fn window() -> Bounds {
    bounds("2026-01-01T00:00:00Z", "2027-01-01T00:00:00Z").unwrap()
}

pub fn request(limit: i64, cursor: StatsCursorArg) -> StatsRequest {
    StatsRequest {
        bounds: Some(window()),
        limit,
        cursor,
    }
}

pub fn done(id: &str, n: i64) -> StatFacts {
    stat_facts(
        id,
        "completed",
        FIRST + n * 1_000_000,
        Some(FIRST + n * 1_000_000 + 5_000_000),
    )
}

pub fn world(facts: Vec<StatFacts>) -> World {
    let mut world = World::new();
    world.feed.rows = facts.into_iter().map(row).collect();
    world
}

pub fn run(world: &World, request: &StatsRequest, sizes: &Sizes) -> Result<StatsOut, ReadError> {
    statistics(&world.reads(NONE_MARKERS), &root(), request, sizes)
}

pub fn bound(generation: Option<&str>, offset: u64) -> StatsCursorArg {
    StatsCursorArg::Bound {
        generation: generation.map(str::to_string),
        offset,
    }
}

pub fn coded(code: &'static str, message: &str) -> ReadError {
    ReadError::Coded {
        code,
        message: message.into(),
    }
}

pub fn ready(out: Result<StatsOut, ReadError>) -> hf_application::ctl::task_reads::StatsPage {
    match out.unwrap() {
        StatsOut::Ready(page) => page,
        other => panic!("{other:?}"),
    }
}

pub fn unavailable(out: Result<StatsOut, ReadError>) -> Vec<(String, String)> {
    match out.unwrap() {
        StatsOut::Unavailable(found) => found.into_iter().map(|d| (d.code, d.message)).collect(),
        other => panic!("{other:?}"),
    }
}

pub fn diag(code: &str, message: &str) -> Vec<(String, String)> {
    vec![(code.into(), message.into())]
}
