//! Historical interval observations, never recovery or execution authority.
use crate::{
    capture::{CaptureSnapshot, FileIdentity},
    ownership::{self, Snapshot},
    process::ProcessIdentity,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceStamp {
    pub generation: ownership::Id,
    pub root: FileIdentity,
    pub database: FileIdentity,
    pub marker: FileIdentity,
    pub store_lock: FileIdentity,
    pub io_lock: FileIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRecord {
    pub source: SourceStamp,
    pub ownership: Snapshot,
    pub capture: Option<CaptureSnapshot>,
    pub events_count: u32,
    pub events_digest: ownership::Digest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Present,
    Zombie,
    AbsentPid,
    PidReused,
    BootChanged,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessEvidence {
    pub expected: ProcessIdentity,
    pub state: ProcessState,
    pub actual: Option<ProcessIdentity>,
    pub boot: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamObservation {
    Present { identity: FileIdentity, length: u64 },
    Missing,
    Unavailable,
    Changed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEvidence {
    pub stdout: StreamObservation,
    pub stderr: StreamObservation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureAssessment {
    NotRecorded,
    Incomplete,
    UnindexedTail,
    Unqualified,
    RecordedTerminalExtentObserved,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReservationDisposition {
    Retained,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryReport {
    pub record: RecoveryRecord,
    pub owner: Option<ProcessEvidence>,
    pub child: Option<ProcessEvidence>,
    pub raw: Option<RawEvidence>,
    pub capture: CaptureAssessment,
    pub reservation: ReservationDisposition,
}
