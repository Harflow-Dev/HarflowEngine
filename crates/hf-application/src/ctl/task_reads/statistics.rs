//! `statistics-page`: finished task executions of a window, paged under one generation.
//!
//! The read has a scan phase (every valid task's facts, in keyset batches) and a projection
//! phase (ancestry, relevance, paging). A scan that cannot finish expires a cursor walk and
//! otherwise leaves the page unavailable; a projection that is incomplete or oversized
//! leaves it unavailable either way.
use super::{
    ReadError, Reads, StoreFailure,
    feed_ports::{FeedPort, ScanFailure, ScanRow},
    gate::{Entry, Family, Observer, authorize, check_damage, enter},
    statistics_page::{
        StatsMeta, StatsOut, StatsPage, StatsSizing, bad_order, coded, page, unavailable,
    },
};
use hf_domain::ctl::{
    caller::{GatePath, marker_refusal},
    state_root::StateRoot,
    statistics::{Diagnostic, KNOWN_STATUSES, StatSource, generation_entries, project},
    statistics_time::Bounds,
};
use std::time::{Duration, Instant};

/// Python's foreground budget of a statistics read, from the start of the command.
pub const STATISTICS_BUDGET: Duration = Duration::from_secs(2);
/// How long before the session deadline the statistics budget is capped, so that its expiry
/// is always judged before the session's and never reads as a busy store.
pub const SESSION_MARGIN: Duration = Duration::from_millis(250);

/// The generation of an empty projection: SHA-256 of `["rs-statistics-1",null,[]]`.
pub const EMPTY_GENERATION: &str =
    "20fd013481cc79458c65358246d976f939e27afcf80cf6bb4996b48070ae21d3";
const SOURCE_LIMIT: usize = 10_000;
/// Task rows of any validity the scan will count before it reads one.
const RAW_ENTRY_LIMIT: usize = 100_000;
const FAILURE_LIMIT: usize = 16;

/// What a bound cursor claims: the generation it was issued under and where it stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Claim {
    generation: Option<String>,
    offset: u64,
}

/// What the cursor of a request is, judged against the bounds of the request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatsCursorArg {
    Absent,
    Malformed,
    /// Well formed, but bound to another state root or window.
    Foreign,
    /// `generation` is `None` when the cursor stores something other than text.
    Bound {
        generation: Option<String>,
        offset: u64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct StatsRequest {
    /// `None` unless both bounds are qualified instants in order.
    pub bounds: Option<Bounds>,
    pub limit: i64,
    pub cursor: StatsCursorArg,
}

/// Bounds and limit, then the cursor, then its binding.
fn prepared(request: &StatsRequest) -> Result<(Bounds, usize, Option<Claim>), ReadError> {
    let limit = usize::try_from(request.limit)
        .ok()
        .filter(|n| (1..=100).contains(n));
    let (Some(bounds), Some(limit)) = (request.bounds, limit) else {
        let message = "Statistics require ordered timezone-qualified bounds and limit 1..100";
        return Err(ReadError::InvalidParams(message.to_string()));
    };
    let old = match &request.cursor {
        StatsCursorArg::Absent => None,
        StatsCursorArg::Malformed => {
            return Err(coded("invalid_cursor", "Invalid statistics cursor"));
        }
        StatsCursorArg::Foreign => {
            let message = "Statistics cursor belongs to another query or state root";
            return Err(coded("invalid_cursor", message));
        }
        StatsCursorArg::Bound { generation, offset } => Some(Claim {
            generation: generation.clone(),
            offset: *offset,
        }),
    };
    Ok((bounds, limit, old))
}

fn expired_history() -> ReadError {
    coded(
        "snapshot_expired",
        "Retained statistics history changed; restart the page walk",
    )
}

/// A root with no store, or no relevant task: the empty ready page.
fn empty_page(old: &Option<Claim>) -> Result<StatsOut, ReadError> {
    if let Some(claim) = old {
        if claim.generation.as_deref() != Some(EMPTY_GENERATION) {
            return Err(expired_history());
        }
        if claim.offset > 0 {
            return Err(bad_order());
        }
    }
    Ok(StatsOut::Ready(StatsPage {
        meta: StatsMeta {
            generation: EMPTY_GENERATION.to_string(),
            eligible: 0,
            diagnostics: Vec::new(),
        },
        items: Vec::new(),
        next_cursor: None,
        has_more: false,
    }))
}

/// Why the scan did not produce sources.
enum Scan {
    Sources(Vec<StatSource>),
    /// The scan cannot finish: the page is unavailable, or a cursor walk expired.
    Incomplete(Diagnostic),
    /// The budget ran out with `read` rows classified, `failures` among them (at most 16).
    Pending {
        read: usize,
        failures: Vec<Diagnostic>,
    },
}

/// The deadlines of a statistics read, and the debug seam that expires its budget early.
#[derive(Clone, Copy)]
struct Budget {
    statistics: Instant,
    session: Instant,
    expire_after_rows: Option<usize>,
}

impl Budget {
    /// Whether the debug seam has counted the budget as spent with `read` rows read. This is
    /// the only check that may stop inside a batch.
    fn seam_spent(&self, read: usize) -> bool {
        self.expire_after_rows.is_some_and(|n| read >= n)
    }

    /// Whether the budget is spent with `read` rows read: the clock or the seam. Checked
    /// between batches only, so every row storage returned is classified.
    fn spent(&self, read: usize) -> bool {
        Instant::now() >= self.statistics || self.seam_spent(read)
    }
}

/// A row as the projection sees it: a status this engine does not know, an oversized row and
/// an oversized qualified payload are unreadable sources.
fn classified(row: ScanRow, sizing: &dyn StatsSizing) -> StatSource {
    match row {
        ScanRow::Oversized(id) => StatSource::Unreadable {
            id,
            code: "source_oversized",
        },
        ScanRow::Facts(facts) if !KNOWN_STATUSES.contains(&facts.status.as_str()) => {
            StatSource::Unreadable {
                id: facts.id.as_str().to_string(),
                code: "source_unavailable",
            }
        }
        ScanRow::Facts(facts) if sizing.source_oversized(&facts) => StatSource::Unreadable {
            id: facts.id.as_str().to_string(),
            code: "projection_oversized",
        },
        ScanRow::Facts(facts) => StatSource::Task(facts),
    }
}

fn scan(feeds: &dyn FeedPort, budget: Budget, sizing: &dyn StatsSizing) -> Result<Scan, ReadError> {
    let incomplete =
        |code: &str, message: &str| Ok(Scan::Incomplete(Diagnostic::new(code, message)));
    let counted = feeds
        .history_rows(RAW_ENTRY_LIMIT + 1)
        .map_err(ReadError::from)?;
    if counted > RAW_ENTRY_LIMIT {
        return incomplete("history_limit", "Raw history name limit exceeded");
    }
    let valid = feeds
        .valid_rows(SOURCE_LIMIT + 1)
        .map_err(ReadError::from)?;
    if valid > SOURCE_LIMIT {
        return incomplete("history_limit", "Statistics source history limit exceeded");
    }
    let mut sources: Vec<StatSource> = Vec::new();
    let mut failures: Vec<Diagnostic> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let stop = |sources: &Vec<StatSource>, failures: Vec<Diagnostic>| {
            Ok(Scan::Pending {
                read: sources.len(),
                failures,
            })
        };
        if budget.spent(sources.len()) {
            return stop(&sources, failures);
        }
        let batch = match feeds.statistics_batch(after.as_deref(), budget.statistics) {
            Ok(batch) => batch,
            Err(ScanFailure::Deadline) if Instant::now() >= budget.session => {
                return Err(ReadError::Store(StoreFailure::Busy));
            }
            Err(ScanFailure::Deadline) => return stop(&sources, failures),
            Err(ScanFailure::Limit) => {
                let message = "Statistics source exceeds the bounded read budget";
                return incomplete("source_oversized", message);
            }
            Err(ScanFailure::Store(failure)) => return Err(ReadError::Store(failure)),
        };
        if batch.rows.is_empty() && !batch.expired {
            return Ok(Scan::Sources(sources));
        }
        if let Some(last) = batch.rows.last() {
            after = Some(last.id().to_string());
        }
        for row in batch.rows {
            if budget.seam_spent(sources.len()) {
                return stop(&sources, failures);
            }
            let source = classified(row, sizing);
            if let StatSource::Unreadable { id, code } = &source
                && failures.len() < FAILURE_LIMIT
            {
                failures.push(Diagnostic::new(code, &format!("task:{id}")));
            }
            sources.push(source);
        }
        if batch.expired {
            return stop(&sources, failures);
        }
        if sources.len() > SOURCE_LIMIT {
            return incomplete("history_limit", "Statistics source history limit exceeded");
        }
    }
}

/// A budget that ran out during the scan, as the reference reports an unfinished
/// classification: no row read leaves the page unavailable; otherwise the page is `preparing`
/// with the failures of the rows read, at most 16, in id order. The ids of the rows not yet
/// read are not listed: the reference leaves them out of its diagnostics too.
fn pending(read: usize, failures: Vec<Diagnostic>) -> StatsOut {
    if read == 0 {
        let message = "No source classification fits the foreground deadline";
        return unavailable("discovery_budget", message);
    }
    StatsOut::Preparing(failures)
}

pub fn statistics(
    reads: &Reads<'_>,
    root: &StateRoot,
    request: &StatsRequest,
    sizing: &dyn StatsSizing,
) -> Result<StatsOut, ReadError> {
    statistics_within(reads, root, request, sizing, STATISTICS_BUDGET)
}

/// As `statistics` with a chosen budget.
pub fn statistics_within(
    reads: &Reads<'_>,
    root: &StateRoot,
    request: &StatsRequest,
    sizing: &dyn StatsSizing,
    budget: Duration,
) -> Result<StatsOut, ReadError> {
    statistics_limited(reads, root, request, sizing, budget, None)
}

/// As `statistics`, with the budget counted from now and capped before the session deadline.
/// `expire_after_rows` ends the budget once that many rows are read; it never extends it.
pub fn statistics_limited(
    reads: &Reads<'_>,
    root: &StateRoot,
    request: &StatsRequest,
    sizing: &dyn StatsSizing,
    budget: Duration,
    expire_after_rows: Option<usize>,
) -> Result<StatsOut, ReadError> {
    let session = reads.deadline;
    let cap = session.checked_sub(SESSION_MARGIN).unwrap_or(session);
    let limits = Budget {
        statistics: Instant::now()
            .checked_add(budget)
            .map_or(cap, |end| end.min(cap)),
        session,
        expire_after_rows,
    };
    if let Some(kind) = marker_refusal(GatePath::DirectOnly, reads.markers) {
        return Err(ReadError::Refused(kind));
    }
    let (marker, view) = match enter(reads, root, GatePath::DirectOnly)? {
        Entry::NoStore => return empty_page(&prepared(request)?.2),
        Entry::Store { marker, view } => (marker, view),
    };
    let mut observer = Observer::new(reads.process);
    let mut out = None;
    reads
        .store
        .with_snapshot(root, reads.deadline, &mut |snapshot| {
            check_damage(snapshot)?;
            let path = GatePath::DirectOnly;
            authorize(
                path,
                snapshot,
                marker,
                &view,
                Family::Complete,
                &mut observer,
            )?;
            let (bounds, limit, old) = prepared(request)?;
            out = Some(read(snapshot, limits, &bounds, limit, &old, sizing)?);
            Ok(())
        })?;
    out.ok_or(ReadError::Internal)
}

fn read(
    feeds: &dyn FeedPort,
    budget: Budget,
    bounds: &Bounds,
    limit: usize,
    old: &Option<Claim>,
    sizing: &dyn StatsSizing,
) -> Result<StatsOut, ReadError> {
    let sources = match scan(feeds, budget, sizing)? {
        Scan::Sources(sources) => sources,
        Scan::Incomplete(_) | Scan::Pending { .. } if old.is_some() => {
            return Err(coded(
                "snapshot_expired",
                "Statistics source coverage changed; restart the page walk",
            ));
        }
        Scan::Incomplete(found) => return Ok(StatsOut::Unavailable(vec![found])),
        Scan::Pending { read, failures } => return Ok(pending(read, failures)),
    };
    let timed_out = || Instant::now() >= budget.statistics;
    let Ok(projection) = project(&sources, bounds, &timed_out) else {
        return Ok(unavailable(
            "discovery_budget",
            "Statistics foreground deadline exhausted",
        ));
    };
    if !projection.complete {
        return Ok(StatsOut::Unavailable(projection.diagnostics));
    }
    if projection.relevant.is_empty() {
        return empty_page(old);
    }
    let entries = generation_entries(&sources, &projection.relevant);
    let generation = feeds
        .statistics_generation(&entries)
        .map_err(ReadError::from)?;
    if let Some(claim) = old
        && claim.generation.as_deref() != Some(generation.as_str())
    {
        return Err(expired_history());
    }
    let meta = StatsMeta {
        generation,
        eligible: projection.items.len(),
        diagnostics: projection.diagnostics,
    };
    page(
        &projection.items,
        meta,
        old.as_ref().map_or(0, |claim| claim.offset),
        limit,
        sizing,
        &timed_out,
    )
}
