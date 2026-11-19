//! Channel pairs and the hooks a writer thread installs for itself.
use super::{
    caller_source::Scripted,
    thread_writer::{Install, ThreadWriter},
};
use super::{ctx::Ctx, harness::Retained, probe::Seed};
use crate::task_runtime::{
    seams::{self},
    writer_service::Runtime as WriterRuntime,
};
use hf_domain::{ownership::OwnerClaim, task_runtime::WriterBootstrap};
use hf_platform::task_runtime::channel::Channel;
use hf_storage::{WalMaintenance, WriterOptions};
use std::{
    any::Any,
    fs::File,
    os::{fd::OwnedFd, unix::net::UnixStream},
    panic::resume_unwind,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
};

/// A connected pair of channels over one socket pair.
pub(super) fn channel_pair() -> (Channel, Channel) {
    let (a, b) = UnixStream::pair().unwrap();
    let end = |stream: UnixStream| {
        let input = File::from(OwnedFd::from(stream));
        let output = input.try_clone().unwrap();
        Channel::new(input, output, 24580).unwrap()
    };
    (end(a), end(b))
}

pub(super) type Retained2 = Arc<dyn Fn(&mut WriterRuntime) + Send + Sync>;
pub(super) type StreamFn = Arc<dyn Fn(&mut crate::task_runtime::stream::Stream) + Send + Sync>;
/// What the writer thread installs for itself.
#[derive(Clone, Default)]
pub(super) struct WriterConfig {
    pub(super) options: Option<Arc<dyn Fn(WriterOptions) -> WriterOptions + Send + Sync>>,
    pub(super) idle:
        Option<Arc<dyn Fn() -> Option<hf_storage::Result<WalMaintenance>> + Send + Sync>>,
    pub(super) retained: Option<Retained2>,
    pub(super) stream: Option<StreamFn>,
    /// A pause before each command the writer answers.
    pub(super) exchange_delay: Option<std::time::Duration>,
}
impl WriterConfig {
    pub(super) fn installer(&self, scripted: Arc<Scripted>, release: Arc<AtomicBool>) -> Install {
        let config = self.clone();
        Box::new(move || {
            use std::sync::atomic::Ordering;
            let mut guards: Vec<Box<dyn Any>> = Vec::new();
            guards.push(Box::new(seams::install_caller(scripted.hook())));
            if let Some(options) = config.options {
                guards.push(Box::new(seams::install_options(Rc::new(move |o| {
                    options(o)
                }))));
            }
            if let Some(stream) = config.stream {
                guards.push(Box::new(seams::install_stream(Rc::new(move |s| stream(s)))));
            }
            if let Some(idle) = config.idle {
                guards.push(Box::new(seams::install_idle(Rc::new(move || idle()))));
            }
            if let Some(delay) = config.exchange_delay {
                guards.push(Box::new(seams::install_delay(Rc::new(move || Some(delay)))));
            }
            let (flag, retained) = (release.clone(), config.retained);
            guards.push(Box::new(seams::install_writer_retained(Rc::new(
                move |rt| {
                    if let Some(observe) = &retained {
                        observe(rt);
                    }
                    if flag.load(Ordering::Acquire) {
                        resume_unwind(Box::new(Retained));
                    }
                },
            ))));
            let flag = release;
            guards.push(Box::new(seams::install_entry_retained(Rc::new(
                move |_| {
                    if flag.load(Ordering::Acquire) {
                        resume_unwind(Box::new(Retained));
                    }
                },
            ))));
            guards
        })
    }
    /// Starts a writer thread for `claim` under the invitation of `seed`.
    pub(super) fn thread(&self, ctx: &Ctx, seed: &Seed, claim: OwnerClaim) -> ThreadWriter {
        let bootstrap = WriterBootstrap {
            root: seed.root.to_str().unwrap().into(),
            invitation: seed.invitation.clone(),
            owner: claim,
            writer: ctx.chain.test.clone(),
        };
        let (owner_end, writer_end) = channel_pair();
        let release = Arc::new(AtomicBool::new(false));
        let install = self.installer(ctx.scripted.clone(), release.clone());
        ThreadWriter::spawn(
            seed.root.clone(),
            bootstrap,
            (writer_end, owner_end),
            release,
            install,
        )
        .unwrap()
    }
}
