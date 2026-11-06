use super::{Slot, TwoTunnelFailureReason as Reason, TwoTunnelOutcome};
use std::{
    io::{self, Read, Write},
    net::Shutdown,
};
pub(super) fn step(
    slots: &mut [Slot],
    index: usize,
    outgoing: bool,
    scratch: &mut [u8; 16384],
    outcome: &mut TwoTunnelOutcome,
) -> Result<(), Reason> {
    let result = advance(slots, index, outgoing, scratch);
    let (_, pending) = totals(slots, outgoing);
    let peak = if outgoing {
        &mut outcome.peak_client_to_upstream_pending
    } else {
        &mut outcome.peak_upstream_to_client_pending
    };
    *peak = (*peak).max(pending);
    result
}
fn advance(
    slots: &mut [Slot],
    index: usize,
    outgoing: bool,
    scratch: &mut [u8; 16384],
) -> Result<(), Reason> {
    let (total, pending) = totals(slots, outgoing);
    let slot = &mut slots[index];
    if slot.finished || slot.reply.is_empty() || slot.reply_offset != slot.reply.len() {
        return Ok(());
    }
    let Some(remote) = slot.remote.as_mut() else {
        return Ok(());
    };
    let (source, target, buffer, stats) = if outgoing {
        (
            &mut slot.client,
            remote,
            &mut slot.outgoing,
            &mut slot.observation.client_to_upstream,
        )
    } else {
        (
            remote,
            &mut slot.client,
            &mut slot.incoming,
            &mut slot.observation.upstream_to_client,
        )
    };
    // One operation per step keeps the caller's poison check before each write.
    if !buffer.is_empty() {
        match target.write(buffer) {
            Ok(0) => return Err(Reason::Io),
            Ok(n) => {
                buffer.drain(..n);
                stats.bytes_forwarded += n;
                stats.buffered_bytes = buffer.len();
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
            Err(_) => return Err(Reason::Io),
        }
        return Ok(());
    }
    if stats.eof {
        if !stats.write_half_closed {
            target.shutdown(Shutdown::Write).map_err(|_| Reason::Io)?;
            stats.write_half_closed = true;
        }
        slot.finished = slot.observation.client_to_upstream.write_half_closed
            && slot.observation.upstream_to_client.write_half_closed;
        return Ok(());
    }
    if total >= 262144 {
        return Err(Reason::ByteBudget);
    }
    // Fixed 8 KiB partitions bound retained storage across both slots.
    let count = (262144 - total).min(16384 - pending).min(8192);
    if count == 0 {
        return Ok(());
    }
    match source.read(&mut scratch[..count]) {
        Ok(0) => stats.eof = true,
        Ok(n) => {
            stats.bytes_read += n;
            if buffer.capacity() == 0 {
                buffer.reserve_exact(8192);
            }
            buffer.extend_from_slice(&scratch[..n]);
            stats.buffered_bytes = buffer.len();
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
        Err(_) => return Err(Reason::Io),
    }
    Ok(())
}

fn totals(slots: &[Slot], outgoing: bool) -> (usize, usize) {
    let total: usize = slots
        .iter()
        .map(|s| {
            if outgoing {
                s.observation.client_to_upstream.bytes_read
            } else {
                s.observation.upstream_to_client.bytes_read
            }
        })
        .sum();
    let pending: usize = slots
        .iter()
        .map(|s| {
            if outgoing {
                s.outgoing.len()
            } else {
                s.incoming.len()
            }
        })
        .sum();
    (total, pending)
}
