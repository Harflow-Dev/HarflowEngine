use super::{super::proxy::relay::route, state::*};
use std::{
    io::{self, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
#[path = "transfer.rs"]
mod transfer;
pub(super) struct Slot {
    client: TcpStream,
    remote: Option<TcpStream>,
    header: Vec<u8>,
    reply: &'static [u8],
    reply_offset: usize,
    preflight: bool,
    finished: bool,
    outgoing: Vec<u8>,
    incoming: Vec<u8>,
    observation: TwoTunnelSlotOutcome,
}
impl Slot {
    fn new(client: TcpStream, ordinal: usize) -> io::Result<Self> {
        client.set_nonblocking(true)?;
        Ok(Self {
            client,
            remote: None,
            header: Vec::new(),
            reply: b"",
            reply_offset: 0,
            preflight: false,
            finished: false,
            outgoing: Vec::new(),
            incoming: Vec::new(),
            observation: TwoTunnelSlotOutcome {
                ordinal,
                ..Default::default()
            },
        })
    }
    fn header_step(
        &mut self,
        target: SocketAddr,
        nonce: &str,
        out: &mut TwoTunnelOutcome,
        until: Instant,
    ) -> Result<(), TwoTunnelFailureReason> {
        if !self.reply.is_empty() || self.finished {
            return Ok(());
        }
        let mut byte = [0];
        for _ in 0..256 {
            match self.client.read(&mut byte) {
                Ok(0) => return Err(TwoTunnelFailureReason::Header),
                Ok(_) => self.header.push(byte[0]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(_) => return Err(TwoTunnelFailureReason::Io),
            }
            if self.header.ends_with(b"\r\n\r\n") {
                return self.admit(target, nonce, out, until);
            }
            if self.header.len() >= 8192 {
                return Err(TwoTunnelFailureReason::Header);
            }
        }
        Ok(())
    }
    fn admit(
        &mut self,
        target: SocketAddr,
        nonce: &str,
        out: &mut TwoTunnelOutcome,
        until: Instant,
    ) -> Result<(), TwoTunnelFailureReason> {
        let text = std::str::from_utf8(&self.header).map_err(|_| TwoTunnelFailureReason::Header)?;
        let connect = route(text, nonce).map_err(|_| TwoTunnelFailureReason::Route)?;
        self.header.clear();
        if !connect {
            if out.preflights != 0 || self.observation.ordinal != 0 {
                return Err(TwoTunnelFailureReason::Preflight);
            }
            out.preflights = 1;
            self.preflight = true;
            self.reply =
                b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\npreflight-ok";
        } else {
            if out.preflights != 1 || self.observation.ordinal == 0 {
                return Err(TwoTunnelFailureReason::Preflight);
            }
            if Instant::now() >= until {
                return Err(TwoTunnelFailureReason::Deadline);
            }
            let remote = TcpStream::connect_timeout(
                &target,
                until
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(10)),
            )
            .map_err(|_| TwoTunnelFailureReason::Io)?;
            if Instant::now() >= until {
                return Err(TwoTunnelFailureReason::Deadline);
            }
            let peer = remote.peer_addr().map_err(|_| TwoTunnelFailureReason::Io)?;
            if peer != target {
                return Err(TwoTunnelFailureReason::Peer);
            }
            self.observation.upstream_local = Some(
                remote
                    .local_addr()
                    .map_err(|_| TwoTunnelFailureReason::Io)?,
            );
            self.observation.upstream_peer = Some(peer);
            remote
                .set_nonblocking(true)
                .map_err(|_| TwoTunnelFailureReason::Io)?;
            self.remote = Some(remote);
            out.connects += 1;
            self.reply = b"HTTP/1.1 200 Connection Established\r\n\r\n";
        }
        Ok(())
    }
    fn reply_step(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        if self.reply_offset < self.reply.len() {
            match self.client.write(&self.reply[self.reply_offset..]) {
                Ok(0) => return Err(io::Error::other("reply closed")),
                Ok(n) => self.reply_offset += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e),
            }
        }
        if self.preflight && self.reply_offset == self.reply.len() {
            self.client.shutdown(Shutdown::Both)?;
            self.finished = true;
        }
        Ok(())
    }
}
struct Context<'a> {
    until: Instant,
    stop: &'a AtomicBool,
    settled: &'a AtomicBool,
    began: Instant,
}
impl Context<'_> {
    fn check(&self, out: &mut TwoTunnelOutcome) -> bool {
        if Instant::now() >= self.until {
            out.fail(TwoTunnelFailureReason::Deadline, None, self.began);
        } else if self.stop.load(Ordering::SeqCst) {
            out.fail(TwoTunnelFailureReason::ControllerStop, None, self.began);
        }
        if out.failed {
            self.settled.store(false, Ordering::Release);
        }
        !out.failed
    }
}
fn complete(slots: &[Slot], out: &TwoTunnelOutcome) -> bool {
    !out.failed
        && out.preflights == 1
        && out.connects > 0
        && out.connects == out.accepted
        && slots.iter().all(|slot| slot.finished)
}
pub(super) fn serve(
    v4: TcpListener,
    v6: TcpListener,
    target: SocketAddr,
    nonce: String,
    until: Instant,
    signals: (&AtomicBool, &AtomicBool),
    seen6: &AtomicBool,
) -> TwoTunnelOutcome {
    let (stop, settled) = signals;
    let context = Context {
        until,
        stop,
        settled,
        began: Instant::now(),
    };
    let mut out = TwoTunnelOutcome::default();
    let mut slots = Vec::new();
    let mut scratch = [0; 16384];
    loop {
        if Instant::now() >= until {
            out.fail(TwoTunnelFailureReason::Deadline, None, context.began);
            break;
        }
        if stop.load(Ordering::SeqCst) && complete(&slots, &out) {
            break;
        }
        if !context.check(&mut out) {
            break;
        }
        settled.store(false, Ordering::Release);
        accept(&v4, &v6, seen6, &mut slots, &mut out, context.began);
        for index in 0..slots.len() {
            if !context.check(&mut out) {
                break;
            }
            if let Err(reason) = slots[index].header_step(target, &nonce, &mut out, until) {
                out.fail(reason, None, context.began);
            }
            if !context.check(&mut out) {
                break;
            }
            if let Err(e) = slots[index].reply_step() {
                out.fail(TwoTunnelFailureReason::Io, Some(e.kind()), context.began);
            }
            for outgoing in [true, false] {
                // Listener/deadline checks precede every payload write/read step.
                settled.store(false, Ordering::Release);
                accept(&v4, &v6, seen6, &mut slots, &mut out, context.began);
                if !context.check(&mut out) {
                    break;
                }
                if let Err((reason, io_kind)) =
                    transfer::step(&mut slots, index, outgoing, &mut scratch, &mut out)
                {
                    out.fail(reason, io_kind, context.began);
                }
            }
        }
        settled.store(complete(&slots, &out), Ordering::Release);
        thread::sleep(Duration::from_millis(1));
    }
    settled.store(false, Ordering::Release);
    accept(&v4, &v6, seen6, &mut slots, &mut out, context.began);
    for mut slot in slots {
        let _ = slot.client.shutdown(Shutdown::Both);
        slot.observation.client_closed = true;
        if let Some(remote) = slot.remote.take() {
            let _ = remote.shutdown(Shutdown::Both);
            drop(remote);
        }
        slot.observation.upstream_closed = true;
        drop(slot.client);
        if slot.observation.ordinal != 0 {
            out.slots.push(slot.observation);
        }
    }
    drop(v4);
    drop(v6);
    out.owned_sockets_closed = true;
    out
}
fn accept(
    v4: &TcpListener,
    v6: &TcpListener,
    seen6: &AtomicBool,
    slots: &mut Vec<Slot>,
    out: &mut TwoTunnelOutcome,
    began: Instant,
) {
    for (listener, ipv6) in [(v4, false), (v6, true)] {
        for _ in 0..4 {
            match listener.accept() {
                Ok((client, _)) => {
                    if ipv6 {
                        seen6.store(true, Ordering::SeqCst);
                        out.ipv6_connections += 1;
                        out.fail(TwoTunnelFailureReason::Ipv6, None, began);
                    } else if out.failed {
                        drop(client);
                    } else {
                        let ordinal = if out.preflights == 0 {
                            0
                        } else {
                            out.accepted += 1;
                            out.accepted
                        };
                        if ordinal > 2 || (ordinal == 0 && !slots.is_empty()) {
                            out.fail(TwoTunnelFailureReason::ExcessConnection, None, began);
                        } else {
                            match Slot::new(client, ordinal) {
                                Ok(slot) => slots.push(slot),
                                Err(e) => {
                                    out.fail(TwoTunnelFailureReason::Io, Some(e.kind()), began)
                                }
                            }
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    out.fail(TwoTunnelFailureReason::Io, Some(e.kind()), began);
                    break;
                }
            }
        }
    }
}
