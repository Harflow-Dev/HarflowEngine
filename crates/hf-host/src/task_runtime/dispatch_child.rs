//! Original owner custody and a one-shot Started handshake.
use super::{Error, HeldRoot, IntentSender};
use hf_domain::{
    process::ProcessIdentity,
    task_runtime::{LaunchIntentCreate, ProductionInvitation},
};
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild, SpawnSpec},
    task_runtime::channel::Channel,
};
use std::{fs::File, path::Path, time::Instant};
struct Custody {
    child: RetainedOwnedChild,
    hold: HeldRoot,
    channel: Option<Channel>,
    error: Option<File>,
}
impl Custody {
    /// This composition is called only by the short-lived ctl executable. The OS
    /// closes leaked descriptors at its immediate exit; this is no settlement proof.
    fn detach_for_ctl_exit(mut self) {
        self.channel.take();
        self.error.take();
        std::mem::forget(self);
    }
    fn transfer(
        &mut self,
        deadline: Instant,
        invitation: &ProductionInvitation,
        intent: LaunchIntentCreate,
        environment: Vec<(String, String)>,
    ) -> Result<String, Error> {
        let identity = self
            .child
            .registered_identity()
            .ok_or(Error::Refused)?
            .clone();
        let (channel, error) = self.child.take_owner_channel(&identity, 24580)?;
        self.channel = Some(channel);
        self.error = Some(error);
        let channel = self.channel.as_mut().ok_or(Error::Refused)?;
        let reply = exchange(
            channel,
            &identity,
            invitation,
            intent,
            environment,
            deadline,
        )?;
        // The root is revalidated after either validated reply, before it is returned.
        self.hold.revalidate()?;
        match reply {
            Reply::Started(task) => Ok(task),
            Reply::NotStarted(reason) => Err(Error::NotStarted(reason)),
        }
    }
}
/// A validated reply from the original owner.
#[cfg_attr(test, derive(Debug))]
pub(super) enum Reply {
    Started(String),
    NotStarted(String),
}
fn send_launch(
    channel: &mut Channel,
    invitation: &ProductionInvitation,
    intent: LaunchIntentCreate,
    environment: Vec<(String, String)>,
    deadline: Instant,
) -> Result<(), Error> {
    let payload =
        hf_interfaces::task_runtime::encode_invitation(invitation).map_err(|_| Error::Refused)?;
    super::io::send(channel, super::io::frame(payload), deadline)?;
    let mut sender = IntentSender::new(intent)?;
    while let Some(frame) = sender.next_frame()? {
        super::io::send(channel, frame, deadline)?;
    }
    super::io::send(
        channel,
        super::io::frame(b"HFTI-END\x01".to_vec()),
        deadline,
    )?;
    let mut sender = super::EnvironmentSender::new(environment)?;
    while let Some(frame) = sender.next_frame()? {
        super::io::send(channel, frame, deadline)?;
    }
    Ok(())
}
pub(super) fn exchange(
    channel: &mut Channel,
    identity: &ProcessIdentity,
    invitation: &ProductionInvitation,
    intent: LaunchIntentCreate,
    environment: Vec<(String, String)>,
    deadline: Instant,
) -> Result<Reply, Error> {
    send_launch(channel, invitation, intent, environment, deadline)?;
    let frame = super::io::receive(channel, deadline)?;
    let payload = super::io::payload(&frame)?;
    if let Ok(refusal) = hf_interfaces::task_runtime::decode_not_started(payload) {
        let e = &refusal.envelope;
        if e.binding != invitation.binding
            || e.owner.process != *identity
            || e.owner.owner != invitation.binding.operation
            || e.owner.nonce != invitation.reservation
            || e.revision != 2
            || e.ownership_revision != 1
            || e.writer == *identity
            || refusal.proof.backend_digest != invitation.backend.digest
            || refusal.proof.child == *identity
            || refusal.proof.child == e.writer
        {
            return Err(Error::Refused);
        }
        return Ok(Reply::NotStarted(refusal.reason));
    }
    let started =
        hf_interfaces::task_runtime::decode_started(payload).map_err(|_| Error::Refused)?;
    let envelope = &started.envelope;
    if envelope.binding != invitation.binding
        || envelope.owner.process != *identity
        || envelope.owner.owner != invitation.binding.operation
        || envelope.owner.nonce != invitation.reservation
        || envelope.revision != 2
        || envelope.ownership_revision != 2
        || started.child == *identity
        || envelope.writer == *identity
        || started.child == envelope.writer
    {
        return Err(Error::Refused);
    }
    Ok(Reply::Started(invitation.binding.task.as_str().into()))
}
pub(super) fn launch(
    root: &Path,
    hold: HeldRoot,
    invitation: ProductionInvitation,
    intent: LaunchIntentCreate,
    environment: Vec<(String, String)>,
    deadline: Instant,
) -> Result<String, Error> {
    hold.revalidate()?;
    let spec = SpawnSpec {
        executable: invitation.ctl.physical_path.clone().into(),
        cwd: std::env::current_dir()?,
        args: vec!["__task-owner".into(), root.as_os_str().into()],
        environment: Vec::new(),
        capture_limit: 16384,
    };
    let child = RetainedOwnedChild::spawn_in_owner_group(spec, &MacIdentity, deadline)?;
    let mut custody = Custody {
        child,
        hold,
        channel: None,
        error: None,
    };
    let result = custody.transfer(deadline, &invitation, intent, environment);
    // No wait, signal, replay or replacement follows either outcome. The original owner
    // independently retains its writer, root exclusion and agent. Close the explicit
    // dispatcher protocol only; retain original wait custody until this process exits.
    custody.detach_for_ctl_exit();
    result
}
