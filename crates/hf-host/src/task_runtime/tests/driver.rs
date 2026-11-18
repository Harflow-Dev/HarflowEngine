//! The test as owner of a writer thread: it sends the commands the owner would send.
use super::harness::{Ctx, Probe, Seed, WriterConfig, deadline};
use super::thread_writer::thread_writer_exit;
use crate::task_runtime::{Error, IntentSender, io};
use hf_domain::{
    ownership::{Digest, OwnerClaim},
    process::{ProcessExit, ProcessIdentity},
    task_runtime::{
        SettlementAttestation, WriterAcknowledgement, WriterCommand, WriterEnvelope,
        WriterOperation,
    },
};
use hf_interfaces::task_runtime::{decode_writer_ack, encode_live_message, encode_writer_command};
use hf_platform::process::{MacIdentity, OwnedSignal, RetainedOwnedChild, SpawnSpec};
use std::time::{Duration, Instant};

pub(super) const SESSION: &str = "123e4567-e89b-42d3-a456-426614174000";

pub(super) fn init_line() -> Vec<u8> {
    format!(
        "{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"{SESSION}\",\"tools\":[{{\"name\":\"Read\"}}]}}\n"
    )
    .into_bytes()
}
pub(super) fn text_line(index: usize) -> Vec<u8> {
    format!(
        "{{\"type\":\"assistant\",\"session_id\":\"{SESSION}\",\"message\":{{\"id\":\"m{index}\",\"content\":[{{\"type\":\"text\",\"text\":\"x\"}}]}}}}\n"
    )
    .into_bytes()
}
pub(super) fn result_line(extra: &str) -> Vec<u8> {
    format!(
        "{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"session_id\":\"{SESSION}\",\"result\":{extra}}}\n"
    )
    .into_bytes()
}

pub(super) struct Driver {
    pub(super) writer: super::thread_writer::ThreadWriter,
    pub(super) envelope: WriterEnvelope,
    pub(super) out: u64,
    pub(super) err: u64,
    pub(super) source: Option<Digest>,
    pub(super) probe: Probe,
    prompt: String,
    child: Option<RetainedOwnedChild>,
    identity: Option<ProcessIdentity>,
}
impl Driver {
    /// Starts the writer thread, waits for T1 and delivers the intent.
    pub(super) fn start(ctx: &Ctx, seed: &Seed, config: &WriterConfig) -> Self {
        let claim = OwnerClaim {
            owner: seed.binding.operation.clone(),
            nonce: seed.invitation.reservation.clone(),
            process: ctx.chain.parent.clone(),
        };
        let mut writer = config.thread(ctx, seed, claim);
        let end = Instant::now() + Duration::from_secs(20);
        let ready = loop {
            if let Some(ready) = writer.step(end).expect("writer ready") {
                break ready;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let envelope = WriterEnvelope {
            binding: ready.binding,
            owner: ready.owner,
            writer: ready.writer,
            revision: ready.revision,
            ownership_revision: ready.ownership_revision,
        };
        let mut driver = Self {
            writer,
            envelope,
            out: 0,
            err: 0,
            source: None,
            probe: seed.probe.clone(),
            prompt: seed.intent.prompt.clone(),
            child: None,
            identity: None,
        };
        let mut sender = IntentSender::new(seed.intent.clone()).unwrap();
        while let Some(frame) = sender.next_frame().unwrap() {
            driver.send_frame(frame);
        }
        driver.send_frame(io::frame(b"HFTI-END\x01".to_vec()));
        let ack = driver.receive_ack().expect("intent acknowledgement");
        assert!(!ack.close_input && !ack.terminal);
        driver
    }
    fn send_frame(&mut self, frame: Vec<u8>) {
        let channel = self.writer.protocol_channel().unwrap();
        io::send(channel, frame, deadline()).unwrap();
    }
    pub(super) fn send(&mut self, operation: WriterOperation) {
        let spawn = u64::from(matches!(operation, WriterOperation::Spawn { .. }));
        let bytes = encode_writer_command(&WriterCommand {
            envelope: self.envelope.clone(),
            operation,
        })
        .unwrap();
        self.send_frame(io::frame(bytes));
        self.envelope.revision += 1;
        self.envelope.ownership_revision += spawn;
    }
    /// One acknowledgement, bounded by `within`.
    pub(super) fn receive_ack_within(
        &mut self,
        within: Duration,
    ) -> Result<WriterAcknowledgement, Error> {
        let channel = self.writer.protocol_channel()?;
        let frame = io::receive(channel, Instant::now() + within)?;
        let ack = decode_writer_ack(io::payload(&frame)?).map_err(|_| Error::Refused)?;
        if ack.envelope != self.envelope {
            return Err(Error::Refused);
        }
        self.source = ack.source_binding.clone().or(self.source.take());
        self.out = ack.stdout_end;
        self.err = ack.stderr_end;
        Ok(ack)
    }
    pub(super) fn receive_ack(&mut self) -> Result<WriterAcknowledgement, Error> {
        self.receive_ack_within(Duration::from_secs(20))
    }
    pub(super) fn command(&mut self, operation: WriterOperation) -> WriterAcknowledgement {
        self.send(operation);
        self.receive_ack().expect("writer acknowledgement")
    }
    /// Spawns a real long-lived child and publishes it as the agent (T2).
    pub(super) fn spawn_child(&mut self) {
        let spec = SpawnSpec {
            executable: "/bin/sleep".into(),
            cwd: std::env::current_dir().unwrap(),
            args: vec!["60".into()],
            environment: Vec::new(),
            capture_limit: 0,
        };
        let child = RetainedOwnedChild::spawn(spec, &MacIdentity, deadline()).unwrap();
        let identity = child.registered_identity().cloned().unwrap();
        self.child = Some(child);
        self.identity = Some(identity.clone());
        self.command(WriterOperation::Spawn {
            child: identity,
            started_at: crate::task_runtime::timestamp().unwrap(),
        });
    }
    pub(super) fn capture(&mut self, bytes: &[u8]) -> WriterAcknowledgement {
        let offset = self.out;
        self.command(WriterOperation::Capture {
            stderr: false,
            offset,
            bytes: bytes.to_vec(),
        })
    }
    pub(super) fn send_capture(&mut self, bytes: &[u8]) {
        let offset = self.out;
        self.send(WriterOperation::Capture {
            stderr: false,
            offset,
            bytes: bytes.to_vec(),
        });
    }
    fn finish_operation(&self, exit: ProcessExit, loss: bool) -> WriterOperation {
        WriterOperation::Finish {
            proof: SettlementAttestation {
                child: self.identity.clone().expect("spawned"),
                exit,
                stdout_end: self.out,
                stderr_end: self.err,
                input_bytes: encode_live_message(&self.prompt).unwrap().len() as u64,
                source_binding: self.source.clone().expect("source binding"),
                capture_loss: loss,
            },
            finished_at: crate::task_runtime::timestamp().unwrap(),
        }
    }
    pub(super) fn finish(&mut self, exit: ProcessExit, loss: bool) -> WriterAcknowledgement {
        let operation = self.finish_operation(exit, loss);
        self.command(operation)
    }
    pub(super) fn send_finish(&mut self, exit: ProcessExit, loss: bool) {
        let operation = self.finish_operation(exit, loss);
        self.send(operation);
    }
    /// Waits for the writer thread to leave its service loop normally.
    pub(super) fn wait_finished(&mut self) -> ProcessExit {
        thread_writer_exit(&mut self.writer, Duration::from_secs(10))
    }
    /// The path of `name` in the task's private raw payload.
    pub(super) fn raw_file(&self, name: &str) -> std::path::PathBuf {
        let b = &self.envelope.binding;
        self.probe
            .root
            .join("private-payload")
            .join(b.generation.as_str())
            .join(b.task.as_str())
            .join(b.attempt.as_str())
            .join(name)
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        if let (Some(child), Some(identity)) = (self.child.as_mut(), self.identity.clone()) {
            let end = Instant::now() + Duration::from_secs(2);
            let _ = child.request_signal(&identity, &MacIdentity, OwnedSignal::Kill, end);
            while child.try_wait().ok() == Some(ProcessExit::Unobserved) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}
