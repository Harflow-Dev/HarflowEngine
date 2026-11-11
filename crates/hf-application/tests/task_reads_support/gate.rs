//! Helpers of the caller-gate tests: the six read paths, their runner and the shared world.
use super::{BOOT, World, page, record, root, task_id};
use hf_application::ctl::task_reads::{
    CursorArg, PageMode, PageRequest, ReadError, list, status, status_batch,
};
use hf_domain::{
    ctl::caller::{EnvMarkers, RefusalKind},
    process::ProcessIdentity,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    Status,
    Batch,
    List,
    Plain,
    Ids,
    Successors,
}

pub const ALL: [Path; 6] = [
    Path::Status,
    Path::Batch,
    Path::List,
    Path::Plain,
    Path::Ids,
    Path::Successors,
];

pub fn request(mode: PageMode) -> PageRequest {
    PageRequest {
        mode,
        limit: 10,
        cursor: CursorArg::Absent,
        active_only: false,
        session: None,
    }
}

pub fn run(world: &World, markers: EnvMarkers, path: Path) -> Result<(), ReadError> {
    let reads = world.reads(markers);
    let first = task_id(1);
    match path {
        Path::Status => status(&reads, &root(), first.as_str()).map(|_| ()),
        Path::Batch => status_batch(&reads, &root(), &[first.as_str().to_string()]).map(|_| ()),
        Path::List => list(&reads, &root(), None).map(|_| ()),
        Path::Plain => page(&reads, &root(), &request(PageMode::Plain)).map(|_| ()),
        Path::Ids => page(
            &reads,
            &root(),
            &request(PageMode::TaskIds(vec![first.as_str().to_string()])),
        )
        .map(|_| ()),
        Path::Successors => page(
            &reads,
            &root(),
            &request(PageMode::Successors(first.as_str().to_string())),
        )
        .map(|_| ()),
    }
}

pub fn world() -> World {
    let mut world = World::new();
    world.tasks.push(record(1, "completed", Some(0), None));
    world
}

pub fn refused(kind: RefusalKind) -> Result<(), ReadError> {
    Err(ReadError::Refused(kind))
}

pub fn stale(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 1, 1, BOOT.to_string()).unwrap()
}
