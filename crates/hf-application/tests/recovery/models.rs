use super::*;

#[test]
fn changed_second_storage_snapshot_is_stale() {
    #[derive(Clone, Copy)]
    enum Change {
        RootIdentity,
        EventDigest,
        EventCount,
    }
    struct ChangingStore {
        value: RecoveryRecord,
        reads: Rc<Cell<u8>>,
        change: Change,
    }
    impl RecoveryStorePort for ChangingStore {
        fn read(&self, _: &Id, _: Instant) -> Result<Option<RecoveryRecord>, RecoveryError> {
            let n = self.reads.get();
            self.reads.set(n + 1);
            let mut value = self.value.clone();
            if n > 0 {
                match self.change {
                    Change::RootIdentity => value.source.root.inode += 1,
                    Change::EventDigest => {
                        value.events_digest = Digest::new("cd".repeat(32)).unwrap();
                    }
                    Change::EventCount => value.events_count += 1,
                }
            }
            Ok(Some(value))
        }
    }
    for change in [
        Change::RootIdentity,
        Change::EventDigest,
        Change::EventCount,
    ] {
        let reads = Rc::new(Cell::new(0));
        let store = ChangingStore {
            value: record(),
            reads: reads.clone(),
            change,
        };
        let inspector = RecoveryInspector::new(store, Identity, Raw { tail: false });
        assert_eq!(
            inspector.inspect(&id(1), Instant::now() + Duration::from_secs(1)),
            Err(RecoveryError::Stale)
        );
        assert_eq!(reads.get(), 2);
    }
}

#[test]
fn unknown_identity_is_valid_evidence_and_never_becomes_dead() {
    struct Unknown;
    impl RecoveryIdentityPort for Unknown {
        fn observe(
            &self,
            expected: &ProcessIdentity,
            _: Instant,
        ) -> Result<ProcessEvidence, RecoveryError> {
            Ok(ProcessEvidence {
                expected: expected.clone(),
                state: ProcessState::Unknown,
                actual: None,
                boot: None,
            })
        }
    }
    let store = Store(record(), Rc::new(Cell::new(0)));
    let inspector = RecoveryInspector::new(store, Unknown, Raw { tail: false });
    let report = inspector
        .inspect(&id(1), Instant::now() + Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(report.owner.unwrap().state, ProcessState::Unknown);
    assert_eq!(report.reservation, ReservationDisposition::Retained);
}

#[test]
fn boot_pid_reuse_and_zombie_models_remain_distinct_observations() {
    for state in [
        ProcessState::Present,
        ProcessState::Zombie,
        ProcessState::PidReused,
        ProcessState::BootChanged,
    ] {
        struct Model(ProcessState);
        impl RecoveryIdentityPort for Model {
            fn observe(
                &self,
                expected: &ProcessIdentity,
                _: Instant,
            ) -> Result<ProcessEvidence, RecoveryError> {
                let (actual, boot) = match self.0 {
                    ProcessState::Present | ProcessState::Zombie => {
                        (Some(expected.clone()), Some(BOOT.to_owned()))
                    }
                    ProcessState::PidReused => (
                        Some(
                            ProcessIdentity::new(
                                expected.pid(),
                                expected.start_seconds() + 1,
                                expected.start_microseconds(),
                                BOOT.into(),
                            )
                            .unwrap(),
                        ),
                        Some(BOOT.to_owned()),
                    ),
                    ProcessState::BootChanged => (
                        None,
                        Some("bbbbbbbb-2345-6789-abcd-ef0123456789".to_owned()),
                    ),
                    _ => unreachable!(),
                };
                Ok(ProcessEvidence {
                    expected: expected.clone(),
                    state: self.0,
                    actual,
                    boot,
                })
            }
        }
        let store = Store(record(), Rc::new(Cell::new(0)));
        let inspector = RecoveryInspector::new(store, Model(state), Raw { tail: false });
        let report = inspector
            .inspect(&id(1), Instant::now() + Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert_eq!(report.owner.unwrap().state, state);
    }
}

#[test]
fn inconsistent_absence_evidence_and_expired_deadline_are_refused() {
    struct Bad;
    impl RecoveryIdentityPort for Bad {
        fn observe(
            &self,
            expected: &ProcessIdentity,
            _: Instant,
        ) -> Result<ProcessEvidence, RecoveryError> {
            Ok(ProcessEvidence {
                expected: expected.clone(),
                state: ProcessState::AbsentPid,
                actual: Some(expected.clone()),
                boot: Some(BOOT.into()),
            })
        }
    }
    let store = Store(record(), Rc::new(Cell::new(0)));
    let inspector = RecoveryInspector::new(store, Bad, Raw { tail: false });
    assert_eq!(
        inspector.inspect(&id(1), Instant::now() + Duration::from_secs(1)),
        Err(RecoveryError::Invalid)
    );

    let reads = Rc::new(Cell::new(0));
    let store = Store(record(), reads.clone());
    let inspector = RecoveryInspector::new(store, Identity, Raw { tail: false });
    assert_eq!(
        inspector.inspect(&id(1), Instant::now() - Duration::from_millis(1)),
        Err(RecoveryError::DeadlineExceeded)
    );
    assert_eq!(reads.get(), 0, "expired calls do not touch injected ports");
}

#[test]
fn stale_capture_fence_refuses_before_any_process_or_raw_observation() {
    let mut stale = record();
    stale.capture.as_mut().unwrap().binding.fence.revision += 1;
    let reads = Rc::new(Cell::new(0));
    let store = Store(stale, reads.clone());
    let inspector = RecoveryInspector::new(store, Identity, Raw { tail: false });
    assert_eq!(
        inspector.inspect(&id(1), Instant::now() + Duration::from_secs(1)),
        Err(RecoveryError::Stale)
    );
    assert_eq!(
        reads.get(),
        1,
        "stale first snapshot prevents downstream observations"
    );
}

#[test]
fn changed_raw_bookend_is_stale_and_late_identity_port_obeys_original_deadline() {
    struct ChangingRaw(Rc<Cell<u8>>);
    impl RecoveryRawPort for ChangingRaw {
        fn observe(
            &self,
            binding: &CaptureBinding,
            progress: &CaptureProgress,
            _: Instant,
        ) -> Result<RawEvidence, RecoveryError> {
            let n = self.0.get();
            self.0.set(n + 1);
            let mut evidence = Raw { tail: false }.observe(binding, progress, Instant::now())?;
            if n > 0
                && let StreamObservation::Present { identity, .. } = &mut evidence.stdout
            {
                identity.inode += 1;
            }
            Ok(evidence)
        }
    }
    let reads = Rc::new(Cell::new(0));
    let inspector = RecoveryInspector::new(
        Store(record(), reads),
        Identity,
        ChangingRaw(Rc::new(Cell::new(0))),
    );
    assert_eq!(
        inspector.inspect(&id(1), Instant::now() + Duration::from_secs(1)),
        Err(RecoveryError::Stale)
    );

    struct Late;
    impl RecoveryIdentityPort for Late {
        fn observe(
            &self,
            expected: &ProcessIdentity,
            deadline: Instant,
        ) -> Result<ProcessEvidence, RecoveryError> {
            while Instant::now() < deadline {
                std::thread::yield_now();
            }
            Ok(ProcessEvidence {
                expected: expected.clone(),
                state: ProcessState::AbsentPid,
                actual: None,
                boot: Some(BOOT.into()),
            })
        }
    }
    let inspector = RecoveryInspector::new(
        Store(record(), Rc::new(Cell::new(0))),
        Late,
        Raw { tail: false },
    );
    assert_eq!(
        inspector.inspect(&id(1), Instant::now() + Duration::from_millis(20)),
        Err(RecoveryError::DeadlineExceeded)
    );
}
