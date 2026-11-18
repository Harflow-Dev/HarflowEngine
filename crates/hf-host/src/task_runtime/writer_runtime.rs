//! Original writer process: entry, service loop and permanent-discard retention.
use super::{Error, WriterEntry, io, writer_service::Runtime};
use hf_interfaces::task_runtime::*;
use std::time::{Duration, Instant};
/// How the service loop ended.
pub(super) enum Served {
    Finished,
    Failed,
}
pub fn run_writer() -> i32 {
    let entry = match WriterEntry::enter(io::deadline()) {
        Ok(entry) => entry,
        Err(_) => return 1,
    };
    run_entry(entry)
}
/// Runs an entered writer: claim, service and, on any failure, permanent retention.
pub(super) fn run_entry(entry: WriterEntry) -> i32 {
    match start(entry) {
        Ok(mut runtime) => match serve(&mut runtime) {
            Served::Finished => 0,
            Served::Failed => retain(&mut runtime),
        },
        Err(entry) => retain_entry(*entry),
    }
}
/// Claims and flushes the entry, then builds the runtime. A failure returns the entry.
pub(super) fn start(mut entry: WriterEntry) -> Result<Runtime, Box<WriterEntry>> {
    if entry.claim_and_queue_ready(io::deadline()).is_err() {
        return Err(Box::new(entry));
    }
    loop {
        match entry.flush_ready() {
            Ok(true) => break,
            Ok(false) => std::thread::sleep(Duration::from_millis(1)),
            Err(_) => return Err(Box::new(entry)),
        }
    }
    // Entry ownership must survive all construction failures: initialize in-place below.
    Runtime::new(entry).map_err(|failure| Box::new(failure.1))
}
pub(super) fn serve(runtime: &mut Runtime) -> Served {
    loop {
        let result = match runtime.entry.channel().receive_step() {
            Ok(hf_platform::task_runtime::channel::Receive::Pending) => {
                match runtime.entry.writer().maintain_idle(Instant::now()) {
                    Ok(()) => {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => Err(error),
                }
            }
            Ok(hf_platform::task_runtime::channel::Receive::Frame(frame)) => {
                decode_writer_command(io::payload(&frame).unwrap_or(&[]))
                    .map_err(|_| Error::Refused)
                    .and_then(|command| runtime.command(command))
            }
            Ok(hf_platform::task_runtime::channel::Receive::Eof) => Err(Error::Refused),
            Err(error) => Err(Error::Io(error)),
        };
        if result.is_err() {
            return Served::Failed;
        }
        if runtime.terminal || runtime.not_started {
            runtime.entry.writer().maintain_final();
            return Served::Finished;
        }
    }
}
fn retain(runtime: &mut Runtime) -> ! {
    runtime.entry.writer().poison();
    if let Some(raw) = &mut runtime.raw {
        raw.poison();
    }
    loop {
        #[cfg(test)]
        super::seams::writer_retained(runtime);
        runtime.entry.channel().discard_step();
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn retain_entry(mut entry: WriterEntry) -> i32 {
    entry.writer().poison();
    loop {
        #[cfg(test)]
        super::seams::entry_retained(&mut entry);
        entry.channel().discard_step();
        std::thread::sleep(Duration::from_millis(1));
    }
}
