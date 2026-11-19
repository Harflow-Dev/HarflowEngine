//! The task writer as a thread of the test process, behind the owner's writer custody.
//!
//! The thread runs the production `run_entry` (T1 claim and ready, then the service loop
//! and permanent retention) over a socket pair. It leaves a retention loop only through the
//! `Retained` unwind, which its release-aware hooks raise once the release flag is set.
use super::harness::Retained;
use crate::task_runtime::{
    Error, HeldRoot, StderrDrain, Writer, WriterDrain, WriterEntry, io, seams::install_diag,
    writer_cause::TagParser, writer_runtime::run_entry,
};
use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    ownership::PreparedRequest,
    process::ProcessExit,
    task_runtime::{WriterBootstrap, WriterReady},
};
use hf_interfaces::task_runtime::{decode_writer_ready, encode_invitation};
use hf_platform::task_runtime::channel::{Channel, Receive};
use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Installs the thread's hooks inside the thread; the returned guards live as long as it.
pub(in crate::task_runtime) type Install = Box<dyn FnOnce() -> Vec<Box<dyn Any>> + Send>;

pub(in crate::task_runtime) struct ThreadWriter {
    channel: Option<Channel>,
    handle: Option<JoinHandle<Option<i32>>>,
    release: Arc<AtomicBool>,
    exit: Option<ProcessExit>,
    expected: WriterBootstrap,
    ready: Option<WriterReady>,
    /// What the writer thread printed as diagnostics, standing in for its stderr pipe.
    diag: Arc<Mutex<Vec<u8>>>,
    cause: TagParser,
}
impl ThreadWriter {
    /// Starts the writer thread; `channel` is the thread's end, `owner_end` the owner's.
    pub(in crate::task_runtime) fn spawn(
        root: PathBuf,
        bootstrap: WriterBootstrap,
        (writer_end, owner_end): (Channel, Channel),
        release: Arc<AtomicBool>,
        install: Install,
    ) -> std::io::Result<Self> {
        let expected = bootstrap.clone();
        let diag = Arc::new(Mutex::new(Vec::new()));
        let sink = diag.clone();
        let handle = std::thread::Builder::new()
            .name("task-writer-thread".into())
            .spawn(move || body(&root, bootstrap, writer_end, install, sink))?;
        Ok(Self {
            channel: Some(owner_end),
            handle: Some(handle),
            release,
            exit: None,
            expected,
            ready: None,
            diag,
            cause: TagParser::default(),
        })
    }
    /// The source tag the writer thread printed, as the owner's stderr collection reads it.
    pub(in crate::task_runtime) fn collect_stderr(&mut self) -> Option<&'static str> {
        let bytes = std::mem::take(&mut *self.diag.lock().unwrap());
        self.cause.feed(&bytes);
        self.cause.tag()
    }
    /// One bounded receive of the ready frame, validated as the link validates it.
    pub(in crate::task_runtime) fn step(
        &mut self,
        deadline: Instant,
    ) -> Result<Option<WriterReady>, Error> {
        if let Some(ready) = &self.ready {
            return Ok(Some(ready.clone()));
        }
        if Instant::now() >= deadline {
            return Err(Error::Poisoned);
        }
        let channel = self.channel.as_mut().ok_or(Error::Refused)?;
        channel.flush_step()?;
        let packet = match channel.receive_step()? {
            Receive::Pending => return Ok(None),
            Receive::Eof => return Err(Error::Refused),
            Receive::Frame(packet) => packet,
        };
        let ready = decode_writer_ready(packet.get(4..).ok_or(Error::Refused)?)
            .map_err(|_| Error::Refused)?;
        let expected = &self.expected;
        if ready.binding != expected.invitation.binding
            || ready.writer != expected.writer
            || ready.owner != expected.owner
            || ready.revision != 1
            || ready.ownership_revision != 1
        {
            return Err(Error::Refused);
        }
        self.ready = Some(ready.clone());
        Ok(Some(ready))
    }
    pub(in crate::task_runtime) fn protocol_channel(&mut self) -> Result<&mut Channel, Error> {
        if self.ready.is_none() {
            return Err(Error::Poisoned);
        }
        self.channel.as_mut().ok_or(Error::Refused)
    }
    /// `Unobserved` while the thread runs; its exit code after it ends.
    pub(in crate::task_runtime) fn try_wait(&mut self) -> std::io::Result<ProcessExit> {
        if self.exit.is_none()
            && let Some(handle) = self.handle.take_if(|h| h.is_finished())
        {
            self.exit = Some(match handle.join() {
                Ok(Some(code)) => ProcessExit::Code(code),
                Ok(None) => ProcessExit::Signal(9),
                Err(payload) => resume_unwind(payload),
            });
        }
        Ok(self.exit.unwrap_or(ProcessExit::Unobserved))
    }
    pub(in crate::task_runtime) fn drain_step(&mut self) -> WriterDrain {
        let ended = self.handle.as_ref().is_none_or(JoinHandle::is_finished);
        WriterDrain {
            stdout: self.channel.as_mut().map(Channel::discard_step),
            stderr: if ended {
                StderrDrain::Eof
            } else {
                StderrDrain::Pending
            },
        }
    }
    /// Closes the owner's channel ends: the writer reads end of file.
    pub(in crate::task_runtime) fn sever(&mut self) {
        self.channel = None;
    }
    /// Closes the channel, raises the release flag and joins the thread within `within`.
    pub(in crate::task_runtime) fn release_and_join(&mut self, within: Duration) {
        self.sever();
        self.release.store(true, Ordering::Release);
        let end = Instant::now() + within;
        while self.handle.as_ref().is_some_and(|h| !h.is_finished()) {
            assert!(
                Instant::now() < end,
                "task-writer-thread did not leave retention within {within:?}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let _ = self.try_wait();
    }
}
impl Drop for ThreadWriter {
    fn drop(&mut self) {
        if self.handle.is_none() {
            return;
        }
        self.sever();
        self.release.store(true, Ordering::Release);
        let end = Instant::now() + Duration::from_secs(5);
        while self.handle.as_ref().is_some_and(|h| !h.is_finished()) {
            if Instant::now() >= end {
                eprintln!("task-writer-thread could not be released; aborting");
                std::process::abort();
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
fn body(
    root: &std::path::Path,
    bootstrap: WriterBootstrap,
    channel: Channel,
    install: Install,
    sink: Arc<Mutex<Vec<u8>>>,
) -> Option<i32> {
    let _diag = install_diag(Rc::new(move |line| {
        sink.lock().unwrap().extend_from_slice(line);
    }));
    let _guards = install();
    let run = catch_unwind(AssertUnwindSafe(|| {
        let entry = open_entry(root, bootstrap, channel);
        entry.map_or(1, run_entry)
    }));
    match run {
        Ok(code) => Some(code),
        Err(payload) if payload.is::<Retained>() => None,
        Err(payload) => resume_unwind(payload),
    }
}
/// The writer's entry as `WriterEntry::enter` builds it, with the injected channel.
fn open_entry(
    root: &std::path::Path,
    bootstrap: WriterBootstrap,
    channel: Channel,
) -> Option<WriterEntry> {
    let deadline = io::deadline();
    let hold = HeldRoot::open(root, &bootstrap.invitation, deadline).ok()?;
    let v = &bootstrap.invitation;
    let payload = encode_invitation(v).ok()?;
    let request = PreparedRequest {
        operation: v.binding.operation.clone(),
        session: v.ownership_session.clone(),
        reservation: v.reservation.clone(),
        attempt: v.binding.attempt.clone(),
        generation: v.binding.generation.clone(),
        digest: hf_storage::Sha256Digest.digest_parts(&[&payload]),
        payload,
    };
    let writer = Writer::open(hold, &request, &bootstrap.owner, deadline).ok()?;
    WriterEntry::for_test(writer, bootstrap, channel).ok()
}

/// Waits up to `within` for the writer thread to end and returns its exit.
pub(in crate::task_runtime) fn thread_writer_exit(
    writer: &mut ThreadWriter,
    within: Duration,
) -> ProcessExit {
    let end = Instant::now() + within;
    loop {
        let exit = writer.try_wait().unwrap();
        if exit != ProcessExit::Unobserved || Instant::now() >= end {
            return exit;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
