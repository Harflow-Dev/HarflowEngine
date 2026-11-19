//! The owner's retained cause: written once before retention, structured, best effort.
use super::{
    ack_loss::retain_with,
    burst_cases::raw_file,
    harness::{Act, Ctx, Script, Seed, WriterConfig, start_owner},
    serial_case,
};
use crate::task_runtime::{
    Error,
    seams::{self, CauseFault, OwnerPoint},
};
use std::{
    cell::RefCell,
    os::unix::fs::{MetadataExt, PermissionsExt},
    rc::Rc,
};

const VERSION: &str = hf_backends::backend_launch::CLAUDE_VERSION;
const STEPS: &str = "read,init,text,result,eofwait,exit:0";
const NOT_RECORDED: &[u8] = b"harflow-owner: retained cause not recorded\n";

/// A failure at T1 that leaves the owner retaining, with its diagnostics collected.
struct Retained {
    _ctx: Ctx,
    seed: Seed,
    diag: Rc<RefCell<Vec<u8>>>,
}
fn retain_after_t1(name: &str, prepare: impl FnOnce(&Ctx, &Seed)) -> Retained {
    let ctx = Ctx::new(name);
    let seed = ctx.seed(STEPS, VERSION);
    prepare(&ctx, &seed);
    let diag = Rc::new(RefCell::new(Vec::new()));
    let sink = diag.clone();
    let _diag = seams::install_diag(Rc::new(move |line| {
        sink.borrow_mut().extend_from_slice(line)
    }));
    let script = Rc::new(Script::default()).on(OwnerPoint::AfterT1, Act::Fail);
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let _owner = seams::install_owner(script.hook());
    let _ = retain_with(&ctx, &seed, &script, case);
    Retained {
        _ctx: ctx,
        seed,
        diag,
    }
}
fn cause_path(seed: &Seed) -> std::path::PathBuf {
    raw_file(seed, "owner-retained.cause")
}

#[test]
fn post_t1_failure_records_retained_cause() {
    let _serial = serial_case();
    let found = Rc::new(RefCell::new(None));
    let record = found.clone();
    let ctx = Ctx::new("cause-t1");
    let seed = ctx.seed(STEPS, VERSION);
    let script = Rc::new(Script::default()).on(OwnerPoint::AfterT1, Act::Fail);
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let _owner = seams::install_owner(script.hook());
    let path = cause_path(&seed);
    // The first retention-loop observation: the record must already exist.
    let (hook, findings) = super::observer::observe(
        ctx.cleanup.clone(),
        std::time::Duration::from_secs(30),
        move |_| *record.borrow_mut() = std::fs::read(&path).ok(),
        |_| Some(()),
    );
    let _retained = seams::install_owner_retained(hook);
    let (outcome, _) = super::harness::with_case(&ctx, case, |case| {
        super::harness::drive_case(&mut case.runtime)
    });
    assert_eq!(outcome, super::harness::Outcome::Retained);
    findings.take();
    let bytes = found
        .borrow()
        .clone()
        .expect("the record exists before retention");
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        "harflow-owner-retained-cause v1\nphase: start\nkind: io\nerrno: -\nsource: -\nend\n"
    );
    let meta = std::fs::metadata(cause_path(&seed)).unwrap();
    assert_eq!(meta.mode() & 0o777, 0o600);
    seed.assert_never_accepted();
}

#[test]
fn writer_start_refusal_records_writer_source_tag() {
    let _serial = serial_case();
    // The raw capture cannot be prepared: its first file already exists.
    let found = retain_after_t1("cause-tag", |_, seed| {
        let dir = raw_file(seed, "x");
        let dir = dir.parent().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        for part in dir.ancestors().take(4) {
            std::fs::set_permissions(part, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::write(dir.join("stdout.raw"), b"").unwrap();
    });
    let text = std::fs::read_to_string(cause_path(&found.seed)).unwrap();
    assert!(text.contains("phase: start\n"), "{text}");
    assert!(text.contains("source: raw_capture_prepare\n"), "{text}");
    assert!(text.ends_with("end\n"));
    assert!(!String::from_utf8_lossy(&found.diag.borrow()).contains("not recorded"));
}

#[test]
fn cause_skipped_when_root_still_group_accessible() {
    let _serial = serial_case();
    let found = retain_after_t1("cause-loose", |ctx, seed| {
        let _ = ctx;
        std::fs::set_permissions(&seed.root, std::fs::Permissions::from_mode(0o755)).unwrap();
    });
    assert!(!cause_path(&found.seed).exists());
    assert_eq!(&*found.diag.borrow(), NOT_RECORDED);
}

#[test]
fn cause_partial_write_lacks_terminator() {
    let _serial = serial_case();
    let _fault = seams::install_cause(Rc::new(|| Some(CauseFault::ShortWrite(20))));
    let found = retain_after_t1("cause-short", |_, _| {});
    let bytes = std::fs::read(cause_path(&found.seed)).unwrap();
    assert_eq!(bytes.len(), 20);
    assert!(!bytes.ends_with(b"end\n"), "the record reads as incomplete");
    assert_eq!(&*found.diag.borrow(), NOT_RECORDED);
}

#[test]
fn cause_write_failure_still_retains() {
    let _serial = serial_case();
    let _fault = seams::install_cause(Rc::new(|| Some(CauseFault::Io)));
    let found = retain_after_t1("cause-io", |_, _| {});
    assert!(!cause_path(&found.seed).exists());
    assert_eq!(&*found.diag.borrow(), NOT_RECORDED);
}

#[test]
fn cause_written_once_per_owner() {
    let _serial = serial_case();
    // An existing record is never replaced.
    let found = retain_after_t1("cause-once", |_, seed| {
        let dir = raw_file(seed, "x");
        let dir = dir.parent().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        for part in dir.ancestors().take(4) {
            std::fs::set_permissions(part, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::write(dir.join("owner-retained.cause"), b"earlier\nend\n").unwrap();
    });
    assert_eq!(
        std::fs::read(cause_path(&found.seed)).unwrap(),
        b"earlier\nend\n"
    );
    assert_eq!(&*found.diag.borrow(), NOT_RECORDED);
}

#[test]
#[cfg(debug_assertions)]
fn a_second_attempt_trips_the_debug_assertion() {
    let _serial = serial_case();
    let ctx = Ctx::new("cause-twice");
    let seed = ctx.seed(STEPS, VERSION);
    let script = Rc::new(Script::default());
    let mut case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    let cause = crate::task_runtime::owner_retain::RetainCause::new(
        crate::task_runtime::owner_retain::Phase::Tick,
        &Error::Refused,
    );
    case.runtime.record_retained_cause(cause);
    let again = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        case.runtime.record_retained_cause(cause);
    }));
    assert!(again.is_err());
    super::harness::with_case(&ctx, case, |_| ());
}

#[test]
fn pre_writer_retention_creates_no_payload_tree_and_no_cause_file() {
    let _serial = serial_case();
    let ctx = Ctx::new("cause-prewriter");
    let seed = ctx.seed(STEPS, VERSION);
    let diag = Rc::new(RefCell::new(Vec::new()));
    let sink = diag.clone();
    let _diag = seams::install_diag(Rc::new(move |line| {
        sink.borrow_mut().extend_from_slice(line)
    }));
    let script = Rc::new(Script::default());
    let case = start_owner(&ctx, &seed, &WriterConfig::default(), &script, None);
    // The writer cannot start: the owner retains before T1.
    let _slot = seams::install_slot(Rc::new(|_, _| Err(Error::Refused)));
    let _owner = seams::install_owner(script.hook());
    let _ = retain_with(&ctx, &seed, &script, case);
    assert!(
        !seed.root.join("private-payload").exists(),
        "no tree before T1"
    );
    assert!(diag.borrow().is_empty(), "silent: nothing was attempted");
}
