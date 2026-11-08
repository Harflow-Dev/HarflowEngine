//! Paging of the statistics projection: items from an offset while they fit the page, the
//! cursor to the next page and the sizes only the renderer knows.
use super::ReadError;
use hf_domain::ctl::statistics::{Diagnostic, StatFacts, StatItem};

const PAGE_BYTES: usize = 256 * 1024;
const CURSOR_BYTES: usize = 4096;

/// What the page reports besides its items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatsMeta {
    pub generation: String,
    pub eligible: usize,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StatsPage {
    pub meta: StatsMeta,
    pub items: Vec<StatItem>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StatsOut {
    Ready(StatsPage),
    Unavailable(Vec<Diagnostic>),
    /// The budget ran out with some rows classified; the diagnostics are their failures.
    Preparing(Vec<Diagnostic>),
}

/// The sizes of the document, which only the renderer knows.
pub trait StatsSizing {
    /// Whether the qualified payload of a task is over its bound.
    fn source_oversized(&self, facts: &StatFacts) -> bool;
    /// Bytes of the document with these items, compact, and no cursor.
    fn candidate(&self, meta: &StatsMeta, items: &[StatItem]) -> usize;
    /// The wire cursor for the next page.
    fn cursor(&self, generation: &str, offset: usize) -> String;
    /// Bytes of the whole document in its output form.
    fn document(&self, meta: &StatsMeta, items: &[StatItem], cursor: Option<&str>) -> usize;
}

/// Items from `offset`, while they fit the page.
pub(super) fn page(
    all: &[StatItem],
    meta: StatsMeta,
    offset: u64,
    limit: usize,
    sizing: &dyn StatsSizing,
    timed_out: &dyn Fn() -> bool,
) -> Result<StatsOut, ReadError> {
    let mut at = usize::try_from(offset).unwrap_or(usize::MAX);
    if at > all.len() {
        return Err(bad_order());
    }
    let mut items: Vec<StatItem> = Vec::new();
    while at < all.len() && items.len() < limit {
        if timed_out() {
            return Ok(unavailable(
                "discovery_budget",
                "Statistics foreground deadline exhausted",
            ));
        }
        items.push(all[at].clone());
        if sizing.candidate(&meta, &items) > PAGE_BYTES - CURSOR_BYTES {
            items.pop();
            if items.is_empty() {
                let message = "One statistics fact exceeds the bounded page";
                return Ok(unavailable("page_limit", message));
            }
            break;
        }
        at += 1;
    }
    let has_more = at < all.len();
    let next_cursor = has_more.then(|| sizing.cursor(&meta.generation, at));
    if next_cursor
        .as_ref()
        .is_some_and(|text| text.len() > CURSOR_BYTES)
    {
        return Ok(unavailable(
            "page_limit",
            "Statistics cursor byte budget exceeded",
        ));
    }
    if timed_out() {
        return Ok(unavailable(
            "discovery_budget",
            "Statistics foreground deadline exhausted",
        ));
    }
    if sizing.document(&meta, &items, next_cursor.as_deref()) > PAGE_BYTES {
        return Ok(unavailable(
            "page_limit",
            "Statistics page byte budget exceeded",
        ));
    }
    if timed_out() {
        return Ok(unavailable(
            "discovery_budget",
            "Statistics foreground deadline exhausted",
        ));
    }
    Ok(StatsOut::Ready(StatsPage {
        meta,
        items,
        next_cursor,
        has_more,
    }))
}

pub(super) fn unavailable(code: &str, message: &str) -> StatsOut {
    StatsOut::Unavailable(vec![Diagnostic::new(code, message)])
}

pub(super) fn coded(code: &'static str, message: &str) -> ReadError {
    ReadError::Coded {
        code,
        message: message.to_string(),
    }
}

pub(super) fn bad_order() -> ReadError {
    coded("invalid_cursor", "Statistics cursor order is invalid")
}
