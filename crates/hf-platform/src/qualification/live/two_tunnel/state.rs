use std::{io, net::SocketAddr, time::Instant};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TwoTunnelFailureReason {
    Deadline,
    ControllerStop,
    Ipv6,
    ExcessConnection,
    Preflight,
    Header,
    Route,
    Io,
    ByteBudget,
    Peer,
}
#[derive(Clone, Debug)]
pub struct TwoTunnelFailure {
    pub reason: TwoTunnelFailureReason,
    pub io_kind: Option<io::ErrorKind>,
    pub elapsed_micros: u128,
}
#[derive(Clone, Debug, Default)]
pub struct TwoTunnelDirection {
    pub bytes_read: usize,
    pub bytes_forwarded: usize,
    pub buffered_bytes: usize,
    pub eof: bool,
    pub write_half_closed: bool,
}
#[derive(Clone, Debug, Default)]
pub struct TwoTunnelSlotOutcome {
    pub ordinal: usize,
    pub upstream_local: Option<SocketAddr>,
    pub upstream_peer: Option<SocketAddr>,
    pub client_to_upstream: TwoTunnelDirection,
    pub upstream_to_client: TwoTunnelDirection,
    pub client_closed: bool,
    pub upstream_closed: bool,
}
#[derive(Clone, Debug, Default)]
pub struct TwoTunnelOutcome {
    pub preflights: usize,
    pub accepted: usize,
    pub connects: usize,
    pub ipv6_connections: usize,
    pub first_failure: Option<TwoTunnelFailure>,
    pub failed: bool,
    pub owned_sockets_closed: bool,
    pub worker_joined: bool,
    pub peak_client_to_upstream_pending: usize,
    pub peak_upstream_to_client_pending: usize,
    pub slots: Vec<TwoTunnelSlotOutcome>,
}
impl TwoTunnelOutcome {
    pub(super) fn fail(
        &mut self,
        reason: TwoTunnelFailureReason,
        kind: Option<io::ErrorKind>,
        began: Instant,
    ) {
        self.failed = true;
        if self.first_failure.is_none() {
            self.first_failure = Some(TwoTunnelFailure {
                reason,
                io_kind: kind,
                elapsed_micros: began.elapsed().as_micros(),
            });
        }
    }
}
