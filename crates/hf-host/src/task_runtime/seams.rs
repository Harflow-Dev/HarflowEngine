//! Fault and observation points that exist only in unit-test builds.
//!
//! Every hook is a thread-local, scoped install: the closure is cloned out of its cell
//! before it is called, so no cell is borrowed while it runs. A hook that is not
//! installed leaves the production behaviour unchanged.
use super::{
    Error, OwnerEntry, WriterEntry, owner_runtime::Runtime as OwnerRuntime, stream::Stream,
    writer_service::Runtime as WriterRuntime, writer_slot::WriterSlot,
};
use hf_domain::{
    ownership::OwnerClaim, prepared_launch::BinaryPin, task_lifecycle::LifecycleBinding,
};
use hf_platform::{process::ReaderPolicy, task_runtime::caller::CallerObservation};
use hf_storage::{WalMaintenance, WriterOptions};
use std::{
    cell::RefCell,
    io,
    rc::Rc,
    thread::LocalKey,
    time::{Duration, Instant},
};

/// Dispatcher-side point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DispatchPoint {
    BeforeIntentRootCheck,
    AfterT0,
}
/// Owner-side points of the start phase.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum OwnerPoint {
    AfterT1,
    AfterT2,
    BeforeStarted,
}
/// How the retained-cause write is made to fail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CauseFault {
    /// The write fails before any file exists.
    Io,
    /// The single write transfers at most this many bytes.
    ShortWrite(usize),
}
type Cell<H> = RefCell<Option<Rc<H>>>;
pub(super) type DispatchHook = dyn Fn(DispatchPoint, &LifecycleBinding) -> Result<(), Error>;
pub(super) type OwnerHook = dyn Fn(OwnerPoint, &mut OwnerRuntime) -> Result<(), Error>;
pub(super) type CallerHook = dyn Fn(u32, Instant) -> io::Result<CallerObservation>;
pub(super) type OwnerRetainedHook = dyn Fn(&mut OwnerRuntime);
pub(super) type WriterRetainedHook = dyn Fn(&mut WriterRuntime);
pub(super) type EntryRetainedHook = dyn Fn(&mut WriterEntry);
pub(super) type StreamHook = dyn Fn(&mut Stream);
pub(super) type CauseHook = dyn Fn() -> Option<CauseFault>;
pub(super) type StallHook = dyn Fn() -> Option<Duration>;
pub(super) type DelayHook = dyn Fn() -> Option<Duration>;
pub(super) type DiagHook = dyn Fn(&[u8]);
pub(super) type OptionsHook = dyn Fn(WriterOptions) -> WriterOptions;
pub(super) type IdleHook = dyn Fn() -> Option<hf_storage::Result<WalMaintenance>>;
pub(super) type PinHook = dyn Fn(Pin) -> Pin;
pub(super) type SlotHook = dyn Fn(&OwnerEntry, &OwnerClaim) -> Result<WriterSlot, Error>;
pub(super) type Pin = Result<BinaryPin, hf_domain::task_launch::LaunchRefusal>;
thread_local! {
    static DISPATCH: Cell<DispatchHook> = const { RefCell::new(None) };
    static OWNER: Cell<OwnerHook> = const { RefCell::new(None) };
    static CALLER: Cell<CallerHook> = const { RefCell::new(None) };
    static OWNER_RETAINED: Cell<OwnerRetainedHook> = const { RefCell::new(None) };
    static WRITER_RETAINED: Cell<WriterRetainedHook> = const { RefCell::new(None) };
    static ENTRY_RETAINED: Cell<EntryRetainedHook> = const { RefCell::new(None) };
    static STREAM: Cell<StreamHook> = const { RefCell::new(None) };
    static CAUSE: Cell<CauseHook> = const { RefCell::new(None) };
    static STALL: Cell<StallHook> = const { RefCell::new(None) };
    static DELAY: Cell<DelayHook> = const { RefCell::new(None) };
    static DIAG: Cell<DiagHook> = const { RefCell::new(None) };
    static OPTIONS: Cell<OptionsHook> = const { RefCell::new(None) };
    static IDLE: Cell<IdleHook> = const { RefCell::new(None) };
    static PIN: Cell<PinHook> = const { RefCell::new(None) };
    static SLOT: Cell<SlotHook> = const { RefCell::new(None) };
}
/// An installed hook; dropping it restores what the cell held before.
pub(super) struct Installed<H: ?Sized + 'static> {
    key: &'static LocalKey<Cell<H>>,
    previous: Option<Rc<H>>,
}
impl<H: ?Sized + 'static> Drop for Installed<H> {
    fn drop(&mut self) {
        let previous = self.previous.take();
        self.key.with(|cell| *cell.borrow_mut() = previous);
    }
}
fn install<H: ?Sized + 'static>(key: &'static LocalKey<Cell<H>>, hook: Rc<H>) -> Installed<H> {
    let previous = key.with(|cell| cell.borrow_mut().replace(hook));
    Installed { key, previous }
}
fn current<H: ?Sized + 'static>(key: &'static LocalKey<Cell<H>>) -> Option<Rc<H>> {
    key.with(|cell| cell.borrow().clone())
}
macro_rules! installer {
    ($name:ident, $key:ident, $hook:ty) => {
        pub(super) fn $name(hook: Rc<$hook>) -> Installed<$hook> {
            install(&$key, hook)
        }
    };
}
installer!(install_dispatch, DISPATCH, DispatchHook);
installer!(install_owner, OWNER, OwnerHook);
installer!(install_caller, CALLER, CallerHook);
installer!(install_owner_retained, OWNER_RETAINED, OwnerRetainedHook);
installer!(install_writer_retained, WRITER_RETAINED, WriterRetainedHook);
installer!(install_entry_retained, ENTRY_RETAINED, EntryRetainedHook);
installer!(install_stream, STREAM, StreamHook);
installer!(install_cause, CAUSE, CauseHook);
installer!(install_stall, STALL, StallHook);
installer!(install_delay, DELAY, DelayHook);
installer!(install_diag, DIAG, DiagHook);
installer!(install_options, OPTIONS, OptionsHook);
installer!(install_idle, IDLE, IdleHook);
installer!(install_pin, PIN, PinHook);
installer!(install_slot, SLOT, SlotHook);

#[inline(never)]
pub(super) fn dispatch_hit(point: DispatchPoint, binding: &LifecycleBinding) -> Result<(), Error> {
    current(&DISPATCH).map_or(Ok(()), |hook| hook(point, binding))
}
#[inline(never)]
pub(super) fn owner_hit(point: OwnerPoint, runtime: &mut OwnerRuntime) -> Result<(), Error> {
    current(&OWNER).map_or(Ok(()), |hook| hook(point, runtime))
}
/// The scripted observation when one is installed, otherwise the real one.
#[inline(never)]
pub(super) fn observe_caller(pid: u32, deadline: Instant) -> io::Result<CallerObservation> {
    match current(&CALLER) {
        Some(hook) => hook(pid, deadline),
        None => hf_platform::task_runtime::caller::observe_caller(pid, deadline),
    }
}
#[inline(never)]
pub(super) fn owner_retained(runtime: &mut OwnerRuntime) {
    if let Some(hook) = current(&OWNER_RETAINED) {
        hook(runtime);
    }
}
#[inline(never)]
pub(super) fn writer_retained(runtime: &mut WriterRuntime) {
    if let Some(hook) = current(&WRITER_RETAINED) {
        hook(runtime);
    }
}
#[inline(never)]
pub(super) fn entry_retained(entry: &mut WriterEntry) {
    if let Some(hook) = current(&ENTRY_RETAINED) {
        hook(entry);
    }
}
#[inline(never)]
pub(super) fn writer_options(base: WriterOptions) -> WriterOptions {
    match current(&OPTIONS) {
        Some(hook) => hook(base),
        None => base,
    }
}
/// An override of the idle maintenance outcome, consulted inside the maintenance closure.
#[inline(never)]
pub(super) fn idle_outcome() -> Option<hf_storage::Result<WalMaintenance>> {
    current(&IDLE).and_then(|hook| hook())
}
#[inline(never)]
pub(super) fn backend_pin(observed: Pin) -> Pin {
    match current(&PIN) {
        Some(hook) => hook(observed),
        None => observed,
    }
}
#[inline(never)]
pub(super) fn writer_slot(
    entry: &OwnerEntry,
    claim: &OwnerClaim,
) -> Option<Result<WriterSlot, Error>> {
    current(&SLOT).map(|hook| hook(entry, claim))
}
/// Called with the stream before and after every consumed batch.
#[inline(never)]
pub(super) fn stream_hook(stream: &mut Stream) {
    if let Some(hook) = current(&STREAM) {
        hook(stream);
    }
}

/// A forced failure of the retained-cause write, when one is installed.
#[inline(never)]
pub(super) fn cause_write_fault() -> Option<CauseFault> {
    current(&CAUSE).and_then(|hook| hook())
}
/// The reader policy of an owner: the production one unless a stall bound is installed.
#[inline(never)]
pub(super) fn reader_policy(base: ReaderPolicy) -> ReaderPolicy {
    current(&STALL)
        .and_then(|hook| hook())
        .map_or(base, ReaderPolicy::with_stall)
}
/// A pause before each command the writer answers, when one is installed.
#[inline(never)]
pub(super) fn exchange_delay() -> Option<Duration> {
    current(&DELAY).and_then(|hook| hook())
}
/// Takes a diagnostic line when a sink is installed; true when it was taken.
#[inline(never)]
pub(super) fn diag_hit(line: &[u8]) -> bool {
    current(&DIAG).is_some_and(|hook| {
        hook(line);
        true
    })
}
