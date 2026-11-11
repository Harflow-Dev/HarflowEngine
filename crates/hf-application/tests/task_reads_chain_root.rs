#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Chain-root qualification and the related-header traversal of the task page: a stored link
//! is validated only when the traversal reaches it.
mod task_reads_support;

use hf_application::ctl::task_reads::{CursorArg, PageMode, PageRequest, ReadError};
use hf_domain::ctl::task_rows::ChainRoot;
use task_reads_support::{NONE_MARKERS, World, page, record, root, task_id};

fn text(value: &str) -> Option<String> {
    Some(value.to_owned())
}

#[test]
fn qualify_separates_absent_valid_invalid_and_empty() {
    assert_eq!(ChainRoot::qualify(None), ChainRoot::Absent);
    let id = task_id(7);
    assert_eq!(
        ChainRoot::qualify(text(id.as_str())),
        ChainRoot::Valid(id.clone())
    );
    for bad in [
        "",
        "not a valid id!",
        "-leading",
        "has space",
        &"a".repeat(65),
    ] {
        assert_eq!(
            ChainRoot::qualify(text(bad)),
            ChainRoot::Invalid(bad.to_owned()),
            "{bad:?}"
        );
    }
    let longest = "a".repeat(64);
    assert!(matches!(
        ChainRoot::qualify(text(&longest)),
        ChainRoot::Valid(_)
    ));
}

#[test]
fn raw_and_valid_expose_the_stored_text_only_where_meaningful() {
    let id = task_id(7);
    let valid = ChainRoot::Valid(id.clone());
    assert_eq!((valid.raw(), valid.valid()), (Some(id.as_str()), Some(&id)));
    let invalid = ChainRoot::Invalid("bad!".into());
    assert_eq!((invalid.raw(), invalid.valid()), (Some("bad!"), None));
    let empty = ChainRoot::Invalid(String::new());
    assert_eq!((empty.raw(), empty.valid()), (Some(""), None));
    assert_eq!(
        (ChainRoot::Absent.raw(), ChainRoot::Absent.valid()),
        (None, None)
    );
}

fn request(ids: &[u32]) -> PageRequest {
    PageRequest {
        mode: PageMode::TaskIds(
            ids.iter()
                .map(|n| task_id(*n).as_str().to_owned())
                .collect(),
        ),
        limit: 10,
        cursor: CursorArg::Absent,
        active_only: false,
        session: None,
    }
}

/// Tasks 1 (root), 2 (continues 1, stores `root`), 3 (unrelated) and 4 (continues 2).
fn world(root_of_two: ChainRoot) -> World {
    let mut world = World::new();
    for n in 1..=4 {
        world.tasks.push(record(n, "completed", Some(0), None));
    }
    world.tasks[1].facts.continuation_of = Some(task_id(1));
    world.tasks[1].facts.chain_root = root_of_two;
    world.tasks[3].facts.continuation_of = Some(task_id(2));
    world
}

fn run(world: &World, ids: &[u32]) -> Result<Vec<String>, ReadError> {
    let out = page(&world.reads(NONE_MARKERS), &root(), &request(ids))?;
    Ok(out
        .related
        .iter()
        .map(|view| view.record.facts.id.as_str().to_owned())
        .collect())
}

#[test]
fn an_invalid_chain_root_on_a_page_row_fails_the_page_with_the_python_repr() {
    for (raw, shown) in [
        ("not a valid id!", "'not a valid id!'"),
        ("it's", "\"it's\""),
    ] {
        let world = world(ChainRoot::Invalid(raw.into()));
        let error = run(&world, &[2]).unwrap_err();
        assert_eq!(
            error,
            ReadError::InvalidParams(format!("not a valid task id: {shown}"))
        );
    }
}

#[test]
fn an_invalid_chain_root_on_a_related_header_fails_when_the_traversal_reaches_it() {
    let world = world(ChainRoot::Invalid("bad!".into()));
    let error = run(&world, &[4]).unwrap_err();
    assert_eq!(
        error,
        ReadError::InvalidParams("not a valid task id: 'bad!'".into())
    );
}

#[test]
fn a_page_that_does_not_contain_the_task_succeeds() {
    let world = world(ChainRoot::Invalid("bad!".into()));
    assert_eq!(run(&world, &[3]).unwrap(), Vec::<String>::new());
    assert_eq!(run(&world, &[1]).unwrap(), Vec::<String>::new());
}

#[test]
fn an_empty_or_absent_stored_root_is_skipped() {
    for stored in [ChainRoot::Invalid(String::new()), ChainRoot::Absent] {
        let world = world(stored);
        assert_eq!(run(&world, &[2]).unwrap(), [task_id(1).as_str()]);
        assert_eq!(
            run(&world, &[4]).unwrap(),
            [task_id(2).as_str(), task_id(1).as_str()]
        );
    }
}

#[test]
fn a_valid_root_is_followed_like_a_continuation_link() {
    let mut world = world(ChainRoot::Valid(task_id(3)));
    world.tasks[1].facts.continuation_of = None;
    assert_eq!(run(&world, &[2]).unwrap(), [task_id(3).as_str()]);
}
