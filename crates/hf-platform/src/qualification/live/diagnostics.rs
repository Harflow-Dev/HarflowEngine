use super::ProxyOutcome;
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyFailureStage {
    ServeAcceptanceGuard,
    ServeAccept,
    ActiveTunnelExtraAccept,
    FinalDrain,
    FinalDrainAccept,
    ClientSetup,
    HeaderRead,
    RouteValidation,
    PreflightValidation,
    PreflightResponse,
    UpstreamSelection,
    UpstreamConnect,
    UpstreamPeer,
    UpstreamSetup,
    ConnectResponse,
    RelayOutgoing,
    RelayIncoming,
    Deadline,
    ControllerStop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProxyFailure {
    pub stage: ProxyFailureStage,
    pub error_kind: Option<io::ErrorKind>,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct RejectionCounts {
    pub serve_acceptance_guard: usize,
    pub active_tunnel_extra_accept: usize,
    pub final_drain: usize,
}

impl ProxyOutcome {
    pub(super) fn failure(&mut self, stage: ProxyFailureStage, error_kind: Option<io::ErrorKind>) {
        self.failed = true;
        if self.first_failure.is_none() {
            self.first_failure = Some(ProxyFailure { stage, error_kind });
        }
    }

    pub(super) fn reject(&mut self, stage: ProxyFailureStage) {
        self.rejected_connections += 1;
        match stage {
            ProxyFailureStage::ServeAcceptanceGuard => {
                self.rejection_counts.serve_acceptance_guard += 1
            }
            ProxyFailureStage::ActiveTunnelExtraAccept => {
                self.rejection_counts.active_tunnel_extra_accept += 1
            }
            ProxyFailureStage::FinalDrain => self.rejection_counts.final_drain += 1,
            _ => (),
        }
        self.failure(stage, None);
    }
}

pub(super) fn observed<T>(
    outcome: &mut ProxyOutcome,
    stage: ProxyFailureStage,
    result: io::Result<T>,
) -> io::Result<T> {
    result.inspect_err(|error| outcome.failure(stage, Some(error.kind())))
}
