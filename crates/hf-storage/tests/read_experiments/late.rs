//! Late corruption through the production queries.
use super::{
    corrupt::{generated, write_at},
    data::Rng,
    ops,
    util::*,
};
use hf_application::event_read::EventRecord;
use std::fs;

const PAGE: u64 = 4096;

/// Complete status and list views as one comparable value.
fn task_view(s: &hf_storage::Store) -> String {
    use hf_domain::task_read::{AuthorizedScope, PageFilter, TaskQuery};
    let dl = ops::deadline;
    let status = TaskQuery::StoredStatus(super::data::task_id(0));
    let page = TaskQuery::page(50, PageFilter::new(false, None).unwrap(), None).unwrap();
    let scope = AuthorizedScope::Global;
    format!(
        "{:?}|{:?}",
        s.task_read(&scope, &status, dl()),
        s.task_read(&scope, &page, dl())
    )
}

fn in_generator(generated: &[Vec<EventRecord>], t: usize, v: &[EventRecord]) -> bool {
    v.iter().all(|r| generated[t].contains(r))
}

/// Late corruption through the production queries: open a qualified store, damage
/// one page while the handle is open, then run every event page query.
pub fn late_corruption() {
    let generated = generated();
    let work = fresh("late-truth");
    let root = work.join("store");
    cp(&dir().join("ci-ckpt"), &root);
    let truth_store = ops::old_open(&root).unwrap();
    let ask = |s: &hf_storage::Store, t: u64| ops::old_events(s, &super::data::task_id(t));
    let truth: Vec<_> = (0..300).map(|t| ask(&truth_store, t)).collect();
    let truth_tasks = task_view(&truth_store);
    drop(truth_store);
    // Independent check of the pristine store: every returned record equals a generated one.
    let from_generator = |t: usize, v: &Vec<EventRecord>| in_generator(&generated, t, v);
    let pristine_consistent = truth
        .iter()
        .enumerate()
        .all(|(t, r)| r.as_ref().is_ok_and(|v| from_generator(t, v)));
    let pages = fs::metadata(root.join("store.sqlite3")).unwrap().len() / PAGE;
    let mut rng = Rng::new(11);
    let mut file = out("late-corruption.jsonl");
    record(
        &mut file,
        &[
            ("kind", "baseline_check".into()),
            (
                "pristine_records_match_generator",
                pristine_consistent.to_string(),
            ),
        ],
    );
    for n in 0..30 {
        let work = fresh("late-trial");
        let root = work.join("store");
        cp(&dir().join("ci-ckpt"), &root);
        let store = ops::old_open(&root).unwrap();
        let page = 2 + rng.below(pages - 2);
        write_at(
            &root.join("store.sqlite3"),
            (page - 1) * PAGE,
            &[0u8; PAGE as usize],
        );
        let results: Vec<_> = (0..300).map(|t| ask(&store, t)).collect();
        let errors = results.iter().filter(|r| r.is_err()).count();
        let wrong = results
            .iter()
            .zip(&truth)
            .filter(|(r, t)| r.is_ok() && r != t)
            .count();
        let not_generated = results
            .iter()
            .enumerate()
            .filter(|(t, r)| r.as_ref().is_ok_and(|v| !from_generator(*t, v)))
            .count();
        let seen = task_view(&store);
        let task_outcome = if seen == truth_tasks {
            "identical"
        } else if seen.contains("Err(") {
            "refused_or_partly_refused"
        } else {
            "ok_but_different"
        };
        record(
            &mut file,
            &[
                ("trial", n.to_string()),
                ("zeroed_page", page.to_string()),
                ("event_page_queries_refused", errors.to_string()),
                (
                    "event_page_queries_ok_but_records_differ_from_pristine",
                    wrong.to_string(),
                ),
                (
                    "event_page_queries_ok_with_records_not_in_generator",
                    not_generated.to_string(),
                ),
                ("status_and_list_views", task_outcome.into()),
            ],
        );
    }
}
