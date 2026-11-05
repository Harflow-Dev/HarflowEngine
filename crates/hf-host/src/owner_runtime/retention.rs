//! Retained original handles. No retry, signal, release, detached join or bounded-kill promise.
use super::actor::Actor;
use hf_domain::process::ProcessExit;
use hf_platform::process::{PrivateFixtureCaptureReader, RetainedOwnedChild};
use hf_storage::RootHold;
use std::time::{Duration, Instant};
pub(super) struct Resources {
    pub(super) writer: Option<RetainedOwnedChild>,
    pub(super) agent: Option<RetainedOwnedChild>,
    pub(super) reader: Option<PrivateFixtureCaptureReader>,
    pub(super) reader_joined: bool,
    pub(super) reader_fifo_reconciled: bool,
    pub(super) consumer_uncertain: (u64, u64),
    pub(super) consumer_abandoned: (u64, u64),
    pub(super) fifo_abandoned: (u64, u64),
    // Last field drops after all original handles and the joined reader.
    pub(super) hold: RootHold,
}
impl Resources {
    pub(super) fn new(hold: RootHold) -> Self {
        Self {
            writer: None,
            agent: None,
            reader: None,
            reader_joined: false,
            reader_fifo_reconciled: false,
            consumer_uncertain: (0, 0),
            consumer_abandoned: (0, 0),
            fifo_abandoned: (0, 0),
            hold,
        }
    }
    pub(super) fn exits(&mut self) -> (ProcessExit, ProcessExit) {
        (exit(&mut self.writer), exit(&mut self.agent))
    }
    pub(super) fn settled(&mut self, actor: Option<&Actor>) -> bool {
        let (writer, agent) = self.exits();
        writer != ProcessExit::Unobserved
            && agent != ProcessExit::Unobserved
            && (self.reader.is_none() || (self.reader_joined && self.reader_fifo_reconciled))
            && actor.is_none_or(|a| a.io.channels_complete())
    }
    /// Unknown work remains resident until positively reaped and channels drained.
    pub(super) fn retain(&mut self, mut actor: Option<&mut Actor>) {
        if let Some(a) = actor.as_deref_mut() {
            a.poison();
        }
        if let Some(w) = self.writer.as_mut() {
            w.close_stdin();
        }
        if let Some(a) = self.agent.as_mut() {
            a.close_stdin();
        }
        if let Some(r) = &self.reader {
            r.discard_capture();
        }
        loop {
            if let Some(a) = actor.as_deref_mut() {
                let _ = a.step();
            } else {
                self.pump_untransferred_writer();
            }
            if self.reader.is_none()
                && let Some(a) = self.agent.as_mut()
            {
                let _ = a.pump(Instant::now() + Duration::from_millis(1), 4096);
            }
            self.reconcile_reader();
            if self.settled(actor.as_deref()) {
                return;
            }
            // No blocking joins; fixed short observation backoff after all owner work.
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn reconcile_reader(&mut self) {
        let Some(r) = self.reader.as_mut() else {
            return;
        };
        if r.is_finished() && !self.reader_joined {
            self.reader_joined = r.join_finished().is_ok();
        }
        // Join happens before the final empty observation, closing producer races.
        for _ in 0..=4 {
            match r.try_recv() {
                Ok(batch) => {
                    self.reader_fifo_reconciled = false;
                    self.fifo_abandoned.0 += batch.stdout.len() as u64;
                    self.fifo_abandoned.1 += batch.stderr.len() as u64;
                }
                Err(_) => {
                    self.reader_fifo_reconciled = self.reader_joined;
                    break;
                }
            }
        }
    }
    pub(super) fn pump_untransferred_writer(&mut self) {
        if let Some(w) = &mut self.writer {
            let _ = w.pump(Instant::now() + Duration::from_millis(1), 4096);
        }
    }
}
fn exit(child: &mut Option<RetainedOwnedChild>) -> ProcessExit {
    child.as_mut().map_or(ProcessExit::Code(0), |c| {
        c.try_wait().unwrap_or(ProcessExit::Unobserved)
    })
}
