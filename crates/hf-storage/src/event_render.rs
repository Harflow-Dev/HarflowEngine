use crate::{
    event_read::Stream,
    event_scan::{Line, Lines},
    read_context::ReadContext,
};
use hf_application::event_read::{EventReadResult, RecordClass};
use hf_domain::{
    event_read::{Direction, EventCursor, EventGap, EventQuery, GapReason},
    task_read::TaskId,
};
pub(crate) fn render(
    context: &ReadContext<'_>,
    query: &EventQuery,
    task: &TaskId,
    stream: &Stream,
    lines: Lines,
    result: &mut EventReadResult,
) {
    let first = lines.rows.iter().position(|l| l.record.seq.is_some());
    let last = lines.rows.iter().rposition(|l| l.record.seq.is_some());
    let neighbour = query.cursor.as_ref().and_then(|c| c.neighbour_seq);
    let skip = query.cursor.as_ref().is_some_and(|c| c.neighbour_skip);
    let older_seq = first.and_then(|i| lines.rows[i].record.seq).or(neighbour);
    let newer_seq = last.and_then(|i| lines.rows[i].record.seq).or(neighbour);
    let older_skip = first.map_or_else(
        || lines.rows.iter().any(hidden) || (skip && lines.direction == Direction::Older),
        |i| lines.rows[..i].iter().any(hidden),
    );
    let newer_skip = last.map_or_else(
        || lines.rows.iter().any(hidden) || (skip && lines.direction == Direction::Newer),
        |i| lines.rows[i + 1..].iter().any(hidden),
    );
    let older = lines.rows.first().map_or(lines.older, |l| l.record.start);
    let newer = lines.rows.last().map_or(lines.newer, |l| l.record.end);
    let older_open = lines
        .rows
        .first()
        .map_or(lines.older_open, |l| l.open_start);
    let newer_open = lines.rows.last().map_or(lines.newer_open, |l| l.open_end);
    let make_cursor = |boundary, direction, resync, neighbour_seq, neighbour_skip| EventCursor {
        store: context.identity.generation.clone(),
        generation: stream.generation.clone(),
        task: task.clone(),
        kinds: query.filter.kinds(),
        direction,
        boundary,
        resync,
        neighbour_seq,
        neighbour_skip,
    };
    result.older_cursor = if older > 0 {
        Some(make_cursor(
            older,
            Direction::Older,
            older_open,
            older_seq,
            older_skip,
        ))
    } else {
        None
    };
    result.newer_cursor = Some(make_cursor(
        newer,
        Direction::Newer,
        newer_open,
        newer_seq,
        newer_skip,
    ));
    result.has_older = older > 0;
    result.has_newer = lines.has_newer;
    result.span = first
        .zip(last)
        .and_then(|(a, b)| lines.rows[a].record.seq.zip(lines.rows[b].record.seq));
    result.gaps = gaps(query, &lines);
    let kinds = query.filter.kinds();
    for line in lines.rows {
        match line.record.class {
            RecordClass::Malformed => result.malformed += 1,
            RecordClass::Oversized => result.oversized += 1,
            RecordClass::Valid if line.record.kind.as_ref().is_some_and(|k| kinds.contains(k)) => {
                result.events.push(line.record)
            }
            _ => {}
        }
    }
}
fn hidden(line: &Line) -> bool {
    matches!(
        line.record.class,
        RecordClass::Malformed | RecordClass::Oversized
    )
}
fn gaps(query: &EventQuery, lines: &Lines) -> Vec<EventGap> {
    let mut gaps = Vec::new();
    let neighbour = query.cursor.as_ref().and_then(|c| c.neighbour_seq);
    let neighbour_skip = query.cursor.as_ref().is_some_and(|c| c.neighbour_skip);
    let mut previous = if lines.direction == Direction::Newer {
        neighbour
    } else {
        None
    };
    let mut skipped = neighbour_skip && lines.direction == Direction::Newer;
    for line in &lines.rows {
        if hidden(line) {
            skipped = true;
        } else if let Some(seq) = line.record.seq {
            add_gap(&mut gaps, previous, seq, skipped);
            previous = Some(seq);
            skipped = false;
        }
    }
    if lines.direction == Direction::Older
        && let Some(seq) = neighbour
    {
        add_gap(&mut gaps, previous, seq, skipped || neighbour_skip);
    }
    gaps.truncate(query.limit);
    gaps
}
fn add_gap(gaps: &mut Vec<EventGap>, previous: Option<u64>, seq: u64, skipped: bool) {
    if let Some(previous) = previous
        && seq > previous.saturating_add(1)
    {
        gaps.push(EventGap {
            after_seq: previous,
            before_seq: seq,
            reason: if skipped {
                GapReason::UnreadableRecords
            } else {
                GapReason::Unknown
            },
        });
    }
}
