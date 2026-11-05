use super::{Measurement, canary, invocation, server};

impl Measurement {
    pub(super) fn finish_candidate(mut self, observation: invocation::VersionObservation) {
        let ipv6_quiet = self.server.no_ipv6_connection();
        let denied_port_quiet = self.denied_listener.no_connection();
        eprintln!(
            "phase=pagesize-settlement ipv6-quiet={ipv6_quiet} denied-port-quiet={denied_port_quiet}"
        );
        let server_outcome = self.server.stop_and_join();
        eprintln!("phase=pagesize-settlement server-outcome={server_outcome:?}");
        assert!(
            ipv6_quiet,
            "localhost scope's IPv6 endpoint received no connection"
        );
        assert!(
            denied_port_quiet,
            "unlisted IPv4 loopback port received no connection"
        );
        assert_eq!(server_outcome, server::ServerOutcome::PreflightOnly);
        canary::verify_and_remove(&self.canary, &self.canary_identity);
        // A known failed version still retains the root and fails the original contract.
        observation.require_supported();
        self.owned.settle();
        eprintln!("phase=vendor-version-only-pagesize complete; messages route was not invoked");
    }
}
