#![allow(dead_code, unused_imports)]
//! Shared scripted world and helpers of the artifact page tests.
use super::task_reads_support::{
    EMPTY_MARKER, FULL_MARKER, NONE_MARKERS, World, caller, identity, items, record, root, task_id,
};

use hf_application::ctl::{
    state_inspect::RootClass,
    task_reads::{
        ArtifactCursorArg, ArtifactEntry, ArtifactPage, ArtifactRequest, ArtifactTargetPort,
        NextCursor, ReadError, Resolution, UnknownReason, artifacts,
    },
};
use hf_domain::ctl::{
    caller::{EnvMarkers, PidObservation, RefusalKind},
    history::{ArtifactItem, Availability, ManifestHead},
};
use std::cell::RefCell;

pub const REVISION: &str = "0123456789abcdef0123456789abcdef";

pub struct Targets {
    pub seen: RefCell<Vec<(String, String)>>,
    pub answer: fn(&ArtifactItem) -> Availability,
    /// A failure that ends the whole command once targets have been observed.
    pub failure: Option<String>,
}

impl ArtifactTargetPort for Targets {
    fn availability(&self, repo_path: &str, item: &ArtifactItem) -> Availability {
        self.seen
            .borrow_mut()
            .push((repo_path.to_string(), item.artifact_id.clone()));
        (self.answer)(item)
    }
    fn command_failure(&self) -> Option<String> {
        self.failure.clone()
    }
}

pub fn targets() -> Targets {
    Targets {
        seen: RefCell::new(Vec::new()),
        failure: None,
        answer: |item| match item.artifact_id.as_str() {
            "x1" => Availability::Changed,
            "x2" => Availability::Deleted,
            "x3" => Availability::Unavailable,
            _ => Availability::Available,
        },
    }
}

pub fn world(count: usize) -> World {
    let mut world = World::new();
    world.history.noisy = true;
    world.tasks.push(record(1, "completed", Some(0), None));
    let key = task_id(1).as_str().to_string();
    world.history.items.insert(key.clone(), items(count));
    world.history.manifests.insert(
        key,
        ManifestHead {
            header_valid: true,
            revision: REVISION.into(),
            count,
        },
    );
    world
}

pub fn request(limit: i64, cursor: ArtifactCursorArg) -> ArtifactRequest {
    ArtifactRequest {
        task_id: task_id(1).as_str().to_string(),
        limit,
        cursor,
        root_task_marker: false,
    }
}

pub fn read(
    world: &World,
    markers: EnvMarkers,
    request: &ArtifactRequest,
) -> Result<ArtifactPage, ReadError> {
    artifacts(
        &world.reads(markers),
        &targets(),
        &root(),
        request,
        &|_, entries| entries.len() * 100,
    )
}

pub fn refused(message: &str) -> Result<ArtifactPage, ReadError> {
    Err(ReadError::InvalidParams(message.to_string()))
}

pub fn bad_cursor() -> Result<ArtifactPage, ReadError> {
    refused("Invalid or stale artifact cursor")
}

pub fn cursor(offset: i64) -> ArtifactCursorArg {
    ArtifactCursorArg::Valid(NextCursor {
        task: task_id(1).as_str().to_string(),
        revision: REVISION.into(),
        offset,
    })
}
