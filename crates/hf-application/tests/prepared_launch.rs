#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "prepared_launch/support.rs"]
mod support;
use hf_application::{prepared_launch::CaptureContext, recovery::RecoveryError};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use support::*;

#[test]
fn missing_original_live_receipt_refuses_before_any_store_or_raw_read() {
    let (invitation, request, _receipt, record, bytes) = fixture();
    let reads = Arc::new(AtomicUsize::new(0));
    let raw_calls = Arc::new(AtomicUsize::new(0));
    let deliveries = Arc::new(AtomicUsize::new(0));
    let reader = reader(record, bytes, reads.clone(), raw_calls.clone(), false);
    let context = CaptureContext {
        invitation: &invitation,
        request: &request,
        source: &source(),
    };
    let error = reader
        .read(
            context,
            &Delivery {
                receipt: None,
                calls: deliveries.clone(),
                timing: None,
            },
            Instant::now() + std::time::Duration::from_secs(1),
        )
        .unwrap_err();
    assert_eq!(error, RecoveryError::Refused);
    assert_eq!(deliveries.load(Ordering::SeqCst), 1);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(raw_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn event_digest_change_between_record_bookends_refuses_projection() {
    let (invitation, request, receipt, record, bytes) = fixture();
    let reads = Arc::new(AtomicUsize::new(0));
    let raw_calls = Arc::new(AtomicUsize::new(0));
    let reader = reader(record, bytes, reads.clone(), raw_calls.clone(), true);
    let context = CaptureContext {
        invitation: &invitation,
        request: &request,
        source: &source(),
    };
    let result = reader.read(
        context,
        &Delivery {
            receipt: Some(receipt),
            calls: Arc::new(AtomicUsize::new(0)),
            timing: None,
        },
        Instant::now() + std::time::Duration::from_secs(1),
    );
    assert_eq!(result.unwrap_err(), RecoveryError::Stale);
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(raw_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn exact_source_budget_starts_after_original_receipt_delivery_deadline() {
    let (invitation, request, receipt, record, bytes) = fixture();
    let timing = Timing::new();
    let reader = timed_reader(record, bytes, timing.clone());
    let src = source();
    let context = CaptureContext {
        invitation: &invitation,
        request: &request,
        source: &src,
    };
    let outer_deadline = Instant::now() + std::time::Duration::from_secs(10);
    let result = reader.read(
        context,
        &Delivery {
            receipt: Some(receipt),
            calls: Arc::new(AtomicUsize::new(0)),
            timing: Some(timing.clone()),
        },
        outer_deadline,
    );
    assert!(result.is_ok());
    assert_eq!(
        *timing.delivery_deadline.lock().unwrap(),
        Some(outer_deadline)
    );
    let delivered = timing.returned.lock().unwrap().unwrap();
    let store_deadlines = timing.store_deadlines.lock().unwrap();
    assert_eq!(store_deadlines.len(), 2);
    let first_store_read = timing.store_called_at.lock().unwrap().unwrap();
    assert!(store_deadlines[0] >= delivered);
    assert!(store_deadlines[0] >= first_store_read);
    assert!(store_deadlines[0] <= first_store_read + std::time::Duration::from_secs(2));
    assert!(store_deadlines[0] < outer_deadline);
    let raw_deadlines = timing.raw_deadlines.lock().unwrap();
    assert_eq!(raw_deadlines.as_slice(), &store_deadlines[..1]);
}

#[test]
fn stale_capture_source_identity_loss_eof_and_event_count_refuse() {
    let (invitation, request, receipt, record, bytes) = fixture();
    let mut variants = Vec::new();
    let mut wrong_id = receipt.clone();
    wrong_id.capture.binding.stdout.inode += 1;
    variants.push((wrong_id, record.clone()));
    let mut loss = receipt.clone();
    loss.capture.progress.loss = true;
    variants.push((loss, record.clone()));
    let mut no_eof = receipt.clone();
    no_eof.capture.progress.stderr_eof = false;
    variants.push((no_eof, record.clone()));
    let mut wrong_phase = receipt.clone();
    wrong_phase.ownership.phase = hf_domain::ownership::Phase::Claimed;
    variants.push((wrong_phase, record.clone()));
    let mut wrong_source = receipt.clone();
    wrong_source.source.root.inode += 1;
    variants.push((wrong_source, record.clone()));
    let mut no_events = record;
    no_events.events_count = 0;
    variants.push((receipt.clone(), no_events));
    for (candidate, stored) in variants {
        let reads = Arc::new(AtomicUsize::new(0));
        let raw = Arc::new(AtomicUsize::new(0));
        let reader = reader(stored, bytes.clone(), reads.clone(), raw.clone(), false);
        let src = source();
        let context = CaptureContext {
            invitation: &invitation,
            request: &request,
            source: &src,
        };
        let result = reader.read(
            context,
            &Delivery {
                receipt: Some(candidate),
                calls: Arc::new(AtomicUsize::new(0)),
                timing: None,
            },
            Instant::now() + std::time::Duration::from_secs(1),
        );
        assert!(
            result.is_err(),
            "invalid receipt or stored event count must refuse"
        );
    }
}
