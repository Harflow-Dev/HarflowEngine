//! Bounded protocol operations retain custody in their caller on every failure.
use super::Error;
use hf_platform::task_runtime::channel::{Channel, Receive};
use std::time::{Duration, Instant};
pub fn frame(payload: Vec<u8>) -> Vec<u8> {
    let mut value = (payload.len() as u32).to_be_bytes().to_vec();
    value.extend(payload);
    value
}
pub fn send(channel: &mut Channel, frame: Vec<u8>, deadline: Instant) -> Result<(), Error> {
    channel.queue(frame)?;
    while !channel.flush_step()? {
        if Instant::now() >= deadline {
            return Err(Error::Poisoned);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
pub fn receive(channel: &mut Channel, deadline: Instant) -> Result<Vec<u8>, Error> {
    loop {
        if Instant::now() >= deadline {
            return Err(Error::Poisoned);
        }
        match channel.receive_step()? {
            Receive::Frame(value) => return Ok(value),
            Receive::Eof => return Err(Error::Refused),
            Receive::Pending => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}
pub fn payload(frame: &[u8]) -> Result<&[u8], Error> {
    frame.get(4..).ok_or(Error::Refused)
}
pub fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
