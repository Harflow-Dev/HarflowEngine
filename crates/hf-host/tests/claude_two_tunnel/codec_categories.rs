//! Closed metadata values; no unrecognized helper text is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Handshake,
    ApplicationRead,
}
impl Phase {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value {
            "handshake" => Ok(Self::Handshake),
            "application_read" => Ok(Self::ApplicationRead),
            _ => Err("invalid helper phase"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    CertificateAlertUnknownCa,
    CertificateAlertBadCertificate,
    CertificateAlertCertificateUnknown,
    PeerAlertInternalError,
    PeerAlertProtocolVersion,
    PeerAlertHandshakeFailure,
    PeerAlertDecodeError,
    PeerAlertIllegalParameter,
    ProtocolWrongVersion,
    ProtocolUnrecognized,
    ProtocolNoSharedCipher,
    ProtocolUnsupportedProtocol,
    LocalCertificateVerificationFailed,
    TransportEof,
    ConnectionReset,
    TransportError,
    TlsUnclassified,
    ApplicationData,
    ObservedEof,
    DeadlineTimeout,
}
impl Category {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value {
            "certificate_alert_unknown_ca" => Ok(Self::CertificateAlertUnknownCa),
            "certificate_alert_bad_certificate" => Ok(Self::CertificateAlertBadCertificate),
            "certificate_alert_certificate_unknown" => Ok(Self::CertificateAlertCertificateUnknown),
            "peer_alert_internal_error" => Ok(Self::PeerAlertInternalError),
            "peer_alert_protocol_version" => Ok(Self::PeerAlertProtocolVersion),
            "peer_alert_handshake_failure" => Ok(Self::PeerAlertHandshakeFailure),
            "peer_alert_decode_error" => Ok(Self::PeerAlertDecodeError),
            "peer_alert_illegal_parameter" => Ok(Self::PeerAlertIllegalParameter),
            "protocol_wrong_version" => Ok(Self::ProtocolWrongVersion),
            "protocol_unrecognized" => Ok(Self::ProtocolUnrecognized),
            "protocol_no_shared_cipher" => Ok(Self::ProtocolNoSharedCipher),
            "protocol_unsupported_protocol" => Ok(Self::ProtocolUnsupportedProtocol),
            "local_certificate_verification_failed" => Ok(Self::LocalCertificateVerificationFailed),
            "transport_eof" => Ok(Self::TransportEof),
            "connection_reset" => Ok(Self::ConnectionReset),
            "transport_error" => Ok(Self::TransportError),
            "tls_unclassified" => Ok(Self::TlsUnclassified),
            "application_data" => Ok(Self::ApplicationData),
            "observed_eof" => Ok(Self::ObservedEof),
            "deadline_timeout" => Ok(Self::DeadlineTimeout),
            _ => Err("invalid helper category"),
        }
    }
    pub fn status(self) -> &'static str {
        match self {
            Self::CertificateAlertUnknownCa
            | Self::CertificateAlertBadCertificate
            | Self::CertificateAlertCertificateUnknown => "cert_alert",
            Self::ApplicationData => "application_data",
            Self::ObservedEof => "eof",
            Self::DeadlineTimeout => "timeout",
            Self::ConnectionReset | Self::TransportError => "io_error",
            _ => "tls_error",
        }
    }
    pub fn permits_phase(self, phase: Phase) -> bool {
        !matches!(self, Self::ApplicationData | Self::ObservedEof)
            || phase == Phase::ApplicationRead
    }
}
