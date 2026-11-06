#[path = "models.rs"]
mod models;
use self::models::*;
pub(super) use self::models::{fixture, source};
use hf_application::{
    owner_runtime::DigestPort,
    prepared_launch::{ExactCaptureReader, ExactRawPort, ReceiptDeliveryPort},
    recovery::{RecoveryError, RecoveryStorePort},
};
use hf_domain::{
    capture::{CaptureBinding, CaptureProgress},
    fixture::Invitation,
    ownership::Id,
    prepared_launch::{CaptureAccepted, RawBytes},
    recovery::RecoveryRecord,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

pub(super) struct Delivery {
    pub(super) receipt: Option<CaptureAccepted>,
    pub(super) calls: Arc<AtomicUsize>,
    pub(super) timing: Option<Arc<Timing>>,
}
pub(super) struct Timing {
    pub(super) delivery_deadline: std::sync::Mutex<Option<Instant>>,
    pub(super) returned: std::sync::Mutex<Option<Instant>>,
    pub(super) store_called_at: std::sync::Mutex<Option<Instant>>,
    pub(super) store_deadlines: std::sync::Mutex<Vec<Instant>>,
    pub(super) raw_deadlines: std::sync::Mutex<Vec<Instant>>,
}
impl Timing {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            delivery_deadline: std::sync::Mutex::new(None),
            returned: std::sync::Mutex::new(None),
            store_called_at: std::sync::Mutex::new(None),
            store_deadlines: std::sync::Mutex::new(Vec::new()),
            raw_deadlines: std::sync::Mutex::new(Vec::new()),
        })
    }
}
impl ReceiptDeliveryPort for Delivery {
    fn receive(
        &self,
        _: &Invitation,
        deadline: Instant,
    ) -> Result<Option<CaptureAccepted>, RecoveryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(timing) = &self.timing {
            *timing.delivery_deadline.lock().unwrap() = Some(deadline);
            *timing.returned.lock().unwrap() = Some(Instant::now());
        }
        Ok(self.receipt.clone())
    }
}
pub(super) struct StoreFake {
    record: RecoveryRecord,
    reads: Arc<AtomicUsize>,
    change_events_on_second: bool,
    timing: Option<Arc<Timing>>,
}
impl RecoveryStorePort for StoreFake {
    fn read(&self, _: &Id, deadline: Instant) -> Result<Option<RecoveryRecord>, RecoveryError> {
        if let Some(timing) = &self.timing {
            timing.store_deadlines.lock().unwrap().push(deadline);
            let mut called = timing.store_called_at.lock().unwrap();
            if called.is_none() {
                *called = Some(Instant::now());
            }
        }
        let count = self.reads.fetch_add(1, Ordering::SeqCst);
        let mut record = self.record.clone();
        if self.change_events_on_second && count == 1 {
            record.events_digest = Hash.digest_parts(&[b"changed-history"]);
        }
        Ok(Some(record))
    }
}
pub(super) struct RawFake {
    bytes: RawBytes,
    calls: Arc<AtomicUsize>,
    timing: Option<Arc<Timing>>,
}
impl ExactRawPort for RawFake {
    fn read_exact(
        &self,
        _: &CaptureBinding,
        _: &CaptureProgress,
        deadline: Instant,
    ) -> Result<RawBytes, RecoveryError> {
        if let Some(timing) = &self.timing {
            timing.raw_deadlines.lock().unwrap().push(deadline);
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.bytes.clone())
    }
}
pub(super) fn reader(
    record: RecoveryRecord,
    bytes: RawBytes,
    reads: Arc<AtomicUsize>,
    raw_calls: Arc<AtomicUsize>,
    change_events: bool,
) -> ExactCaptureReader<StoreFake, RawFake, Hash> {
    ExactCaptureReader::new(
        StoreFake {
            record,
            reads,
            change_events_on_second: change_events,
            timing: None,
        },
        RawFake {
            bytes,
            calls: raw_calls,
            timing: None,
        },
        Hash,
    )
}
pub(super) fn timed_reader(
    record: RecoveryRecord,
    bytes: RawBytes,
    timing: Arc<Timing>,
) -> ExactCaptureReader<StoreFake, RawFake, Hash> {
    ExactCaptureReader::new(
        StoreFake {
            record,
            reads: Arc::new(AtomicUsize::new(0)),
            change_events_on_second: false,
            timing: Some(timing.clone()),
        },
        RawFake {
            bytes,
            calls: Arc::new(AtomicUsize::new(0)),
            timing: Some(timing),
        },
        Hash,
    )
}
