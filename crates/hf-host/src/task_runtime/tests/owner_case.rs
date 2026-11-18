//! Owner scripts, owner cases and the scoped owner drive.
use super::{
    harness::{Ctx, Retained, Seed, WriterConfig, channel_pair, deadline},
    thread_writer::ThreadWriter,
};
use crate::task_runtime::{
    Error, OwnerEntry, dispatch_child,
    owner_runtime::{Runtime, drive},
    seams::{self, OwnerHook, OwnerPoint},
    writer_slot::WriterSlot,
};
use hf_domain::task_runtime::WriterBootstrap;
use std::{
    any::Any,
    cell::RefCell,
    collections::HashMap,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
    thread::JoinHandle,
};

/// Outcome of driving an owner.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Outcome {
    Settled(i32),
    Retained,
}
/// Drives the owner on this thread; a `Retained` unwind is the retention outcome.
pub(super) fn drive_case(runtime: &mut Runtime) -> Outcome {
    match catch_unwind(AssertUnwindSafe(|| drive(runtime))) {
        Ok(code) => Outcome::Settled(code),
        Err(payload) if payload.is::<Retained>() => Outcome::Retained,
        Err(payload) => resume_unwind(payload),
    }
}

/// Scripted owner hook points.
pub(super) enum Act {
    Fail,
    Pause(Box<dyn Fn(&mut Runtime)>),
}
#[derive(Default)]
pub(super) struct Script {
    acts: RefCell<HashMap<OwnerPoint, Act>>,
    arrivals: RefCell<HashMap<OwnerPoint, usize>>,
}
impl Script {
    pub(super) fn on(self: Rc<Self>, point: OwnerPoint, act: Act) -> Rc<Self> {
        self.acts.borrow_mut().insert(point, act);
        self
    }
    pub(super) fn arrivals(&self, point: OwnerPoint) -> usize {
        self.arrivals.borrow().get(&point).copied().unwrap_or(0)
    }
    pub(super) fn hook(self: &Rc<Self>) -> Rc<OwnerHook> {
        let script = self.clone();
        Rc::new(move |point, runtime| {
            *script.arrivals.borrow_mut().entry(point).or_insert(0) += 1;
            match script.acts.borrow().get(&point) {
                Some(Act::Fail) => Err(Error::Io(std::io::Error::other("injected failure"))),
                Some(Act::Pause(pause)) => {
                    pause(runtime);
                    Ok(())
                }
                None => Ok(()),
            }
        })
    }
}
/// One owner under test and its dispatcher.
pub(super) struct OwnerCase {
    pub(super) runtime: Runtime,
    pub(super) dispatcher: Option<JoinHandle<Result<dispatch_child::Reply, Error>>>,
    sender: Option<JoinHandle<()>>,
    _hooks: Vec<Box<dyn Any>>,
}
/// The test as dispatcher: `arrived` is raised by the owner's pause, then the channel is
/// dropped and `release` raised.
pub(super) type Flags = (Arc<AtomicBool>, Arc<AtomicBool>);
/// Builds an owner over a channel pair. With `manual` the test sends the launch frames
/// itself with the production senders and drops its end once the owner has arrived.
pub(super) fn start_owner(
    ctx: &Ctx,
    seed: &Seed,
    config: &WriterConfig,
    script: &Rc<Script>,
    manual: Option<Flags>,
) -> OwnerCase {
    let (dispatcher_end, owner_end) = channel_pair();
    let (dispatcher, sender) = match manual {
        None => (Some(seed.dispatcher(dispatcher_end)), None),
        Some(flags) => (None, Some(seed.manual_dispatcher(dispatcher_end, flags))),
    };
    let mut hooks: Vec<Box<dyn Any>> = vec![
        Box::new(seams::install_caller(ctx.scripted.hook())),
        Box::new(seams::install_owner(script.hook())),
    ];
    let (config, scripted, root) = (config.clone(), ctx.scripted.clone(), seed.root.clone());
    let test = ctx.chain.test.clone();
    let bootstrap_root = root.to_str().unwrap().to_owned();
    hooks.push(Box::new(seams::install_slot(Rc::new(
        move |entry, claim| {
            let bootstrap = WriterBootstrap {
                root: bootstrap_root.clone(),
                invitation: entry.invitation().clone(),
                owner: claim.clone(),
                writer: test.clone(),
            };
            let (owner_end, writer_end) = channel_pair();
            let release = Arc::new(AtomicBool::new(false));
            let install = config.installer(scripted.clone(), release.clone());
            let thread = ThreadWriter::spawn(
                root.clone(),
                bootstrap,
                (writer_end, owner_end),
                release,
                install,
            )?;
            Ok(WriterSlot::Thread(Box::new(thread)))
        },
    ))));
    let entry = OwnerEntry::for_test(&seed.root, seed.owner.clone(), owner_end, deadline())
        .expect("owner entry");
    OwnerCase {
        runtime: Runtime::new(entry),
        dispatcher,
        sender,
        _hooks: hooks,
    }
}
impl OwnerCase {
    /// Cleans up the live runtime, releases the channels and returns the dispatcher's view.
    pub(super) fn finish(self, ctx: &Ctx) -> Option<Result<dispatch_child::Reply, Error>> {
        let Self {
            mut runtime,
            dispatcher,
            sender,
            _hooks,
        } = self;
        ctx.cleanup.owner(&mut runtime);
        drop(runtime);
        if let Some(sender) = sender {
            sender.join().expect("manual dispatcher thread");
        }
        dispatcher.map(|handle| handle.join().expect("dispatcher thread"))
    }
}
/// Runs `body` with the live runtime. A panic of `body` is raised only after cleanup.
pub(super) fn with_case<T>(
    ctx: &Ctx,
    mut case: OwnerCase,
    body: impl FnOnce(&mut OwnerCase) -> T,
) -> (T, Option<Result<dispatch_child::Reply, Error>>) {
    let result = catch_unwind(AssertUnwindSafe(|| body(&mut case)));
    let reply = catch_unwind(AssertUnwindSafe(|| case.finish(ctx)));
    match (result, reply) {
        (Ok(value), Ok(reply)) => (value, reply),
        (Err(payload), _) | (_, Err(payload)) => resume_unwind(payload),
    }
}
