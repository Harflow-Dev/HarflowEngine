//! Read access to one launch's rows and the seeded launch itself.
use super::harness::{Flags, deadline};
use crate::task_runtime::{Error, dispatch_child, io, store_options};
use hf_domain::{
    ownership::PreparedRequest,
    process::ProcessIdentity,
    task_lifecycle::{LifecycleBinding, LifecycleEvidence},
    task_read::{AuthorizedScope, TaskQuery},
    task_runtime::{LaunchIntentCreate, ProductionInvitation},
};
use hf_interfaces::task_runtime::encode_invitation;
use hf_platform::task_runtime::channel::Channel;
use hf_storage::{Limits, Store};
use std::{
    path::PathBuf,
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Read-only access to one launch's rows through separate suppressed readers.
#[derive(Clone)]
pub(super) struct Probe {
    pub(super) root: PathBuf,
    pub(super) binding: LifecycleBinding,
}
impl Probe {
    pub(super) fn store(&self) -> Store {
        open_reader(&self.root)
    }
    pub(super) fn evidence(&self) -> LifecycleEvidence {
        self.store()
            .read_lifecycle(&self.binding, deadline())
            .unwrap()
            .expect("lifecycle row")
    }
    /// The stored task status text, `None` before T2.
    pub(super) fn task_status(&self) -> Option<String> {
        let text = self.status_text()?;
        let start = text.find("\"status\":\"")? + 10;
        Some(text[start..start + text[start..].find('"')?].to_owned())
    }
    /// The served status blob, `None` before T2.
    pub(super) fn status_text(&self) -> Option<String> {
        let result = self
            .store()
            .task_read(
                &AuthorizedScope::Global,
                &TaskQuery::StoredStatus(self.binding.task.clone()),
                deadline(),
            )
            .unwrap();
        let status = result.rows.first()?.status.clone()?;
        Some(String::from_utf8(status).unwrap())
    }
    /// D11: acceptance is never recorded and the task is never cancelled.
    pub(super) fn assert_never_accepted(&self) {
        assert!(
            !self
                .store()
                .launch_acceptance_recorded(&self.binding, deadline())
                .unwrap()
        );
        let evidence = self.evidence();
        assert_ne!(
            evidence.ownership.phase,
            hf_domain::ownership::Phase::AcceptanceRecorded
        );
        assert!(evidence.cancel.is_none());
    }
}
/// One seeded launch: invitation, T0 and the owner identity.
pub(super) struct Seed {
    pub(super) probe: Probe,
    pub(super) invitation: ProductionInvitation,
    pub(super) preparation: PreparedRequest,
    pub(super) intent: LaunchIntentCreate,
    pub(super) env: Vec<(String, String)>,
    pub(super) owner: ProcessIdentity,
}
impl std::ops::Deref for Seed {
    type Target = Probe;
    fn deref(&self) -> &Probe {
        &self.probe
    }
}
impl Seed {
    pub(super) fn manual_dispatcher(&self, channel: Channel, flags: Flags) -> JoinHandle<()> {
        use std::sync::atomic::Ordering;
        let (invitation, intent, env) = (
            self.invitation.clone(),
            self.intent.clone(),
            self.env.clone(),
        );
        std::thread::spawn(move || {
            let mut channel = channel;
            let end = Instant::now() + Duration::from_secs(30);
            let payload = encode_invitation(&invitation).unwrap();
            io::send(&mut channel, io::frame(payload), end).unwrap();
            let mut sender = crate::task_runtime::IntentSender::new(intent).unwrap();
            while let Some(frame) = sender.next_frame().unwrap() {
                io::send(&mut channel, frame, end).unwrap();
            }
            io::send(&mut channel, io::frame(b"HFTI-END\x01".to_vec()), end).unwrap();
            let mut sender = crate::task_runtime::EnvironmentSender::new(env).unwrap();
            while let Some(frame) = sender.next_frame().unwrap() {
                io::send(&mut channel, frame, end).unwrap();
            }
            while !flags.0.load(Ordering::Acquire) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(2));
            }
            drop(channel);
            flags.1.store(true, Ordering::Release);
        })
    }
    pub(super) fn dispatcher(
        &self,
        channel: Channel,
    ) -> JoinHandle<Result<dispatch_child::Reply, Error>> {
        let (owner, invitation) = (self.owner.clone(), self.invitation.clone());
        let (intent, env) = (self.intent.clone(), self.env.clone());
        std::thread::spawn(move || {
            let mut channel = channel;
            let end = Instant::now() + Duration::from_secs(90);
            dispatch_child::exchange(&mut channel, &owner, &invitation, intent, env, end)
        })
    }
}
pub(super) fn open_reader(root: &std::path::Path) -> Store {
    Store::open_with(
        root,
        Limits::default(),
        deadline(),
        hf_platform::lease::shared(),
        store_options::suppressed(),
    )
    .unwrap()
}
