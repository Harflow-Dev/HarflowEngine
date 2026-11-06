"""Capture-free two-session loopback TLS witness; fixed readiness lifetime."""
import os
import selectors
import socket
import ssl
import subprocess
import sys
import time

TLS_REASONS = {
    "TLSV1_ALERT_UNKNOWN_CA": "certificate_alert_unknown_ca",
    "SSLV3_ALERT_CERTIFICATE_UNKNOWN": "certificate_alert_certificate_unknown",
    "TLSV1_ALERT_BAD_CERTIFICATE": "certificate_alert_bad_certificate",
    "SSLV3_ALERT_BAD_CERTIFICATE": "certificate_alert_bad_certificate",
    "TLSV1_ALERT_CERTIFICATE_UNKNOWN": "certificate_alert_certificate_unknown",
    "TLSV1_ALERT_INTERNAL_ERROR": "peer_alert_internal_error",
    "TLSV1_ALERT_PROTOCOL_VERSION": "peer_alert_protocol_version",
    "SSLV3_ALERT_HANDSHAKE_FAILURE": "peer_alert_handshake_failure",
    "TLSV1_ALERT_DECODE_ERROR": "peer_alert_decode_error",
    "SSLV3_ALERT_ILLEGAL_PARAMETER": "peer_alert_illegal_parameter",
    "WRONG_VERSION_NUMBER": "protocol_wrong_version",
    "UNKNOWN_PROTOCOL": "protocol_unrecognized",
    "NO_SHARED_CIPHER": "protocol_no_shared_cipher",
    "UNSUPPORTED_PROTOCOL": "protocol_unsupported_protocol",
}
CERT_CATEGORIES = frozenset({
    "certificate_alert_unknown_ca", "certificate_alert_bad_certificate",
    "certificate_alert_certificate_unknown",
})


def classify_error(error):
    if isinstance(error, ssl.SSLCertVerificationError):
        return "tls_error", "local_certificate_verification_failed", False
    if isinstance(error, ssl.SSLEOFError):
        return "tls_error", "transport_eof", False
    if isinstance(error, ssl.SSLError):
        category = "tls_unclassified"
        reason = getattr(error, "reason", None)
        if getattr(error, "library", None) == "SSL" and isinstance(reason, str):
            category = TLS_REASONS.get(reason, category)
        certificate = category in CERT_CATEGORIES
        return "cert_alert" if certificate else "tls_error", category, certificate
    if isinstance(error, ConnectionResetError):
        return "io_error", "connection_reset", False
    return "io_error", "transport_error", False


def settle(selector, sessions, secure, status, category, certificate=False, application=False):
    ordinal, peer, handshake = sessions.pop(secure)
    phase = "application_read" if handshake else "handshake"
    selector.unregister(secure)
    secure.close()
    print(f"SESSION_V2\t{ordinal}\t{peer[0]}\t{peer[1]}\t{int(certificate)}\t{int(application)}\t{status}\t1\t{phase}\t{category}", flush=True)


def progress(selector, sessions, secure):
    ordinal, peer, handshake = sessions[secure]
    try:
        if not handshake:
            secure.do_handshake()
            sessions[secure] = (ordinal, peer, True)
        data_present = bool(secure.recv(1))
        settle(selector, sessions, secure, "application_data" if data_present else "eof",
               "application_data" if data_present else "observed_eof", application=data_present)
    except ssl.SSLWantReadError:
        selector.modify(secure, selectors.EVENT_READ)
    except ssl.SSLWantWriteError:
        selector.modify(secure, selectors.EVENT_WRITE)
    except OSError as error:
        status, category, certificate = classify_error(error)
        settle(selector, sessions, secure, status, category, certificate)


def serve(context):
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.bind(("127.0.0.1", 0))
    listener.listen(2)
    listener.setblocking(False)
    selector = selectors.DefaultSelector()
    selector.register(listener, selectors.EVENT_READ)
    sessions = {}
    accepted = 0
    deadline = time.monotonic() + 25
    print(f"READY\t{listener.getsockname()[1]}", flush=True)
    while time.monotonic() < deadline and (accepted < 2 or sessions):
        for key, _ in selector.select(max(0, deadline - time.monotonic())):
            if key.fileobj is listener:
                connection, peer = listener.accept()
                accepted += 1
                connection.setblocking(False)
                secure = context.wrap_socket(connection, server_side=True, do_handshake_on_connect=False)
                sessions[secure] = (accepted, peer, False)
                selector.register(secure, selectors.EVENT_READ)
                if accepted == 2:
                    selector.unregister(listener)
                    listener.close()
            else:
                progress(selector, sessions, key.fileobj)
    for secure in list(sessions):
        settle(selector, sessions, secure, "timeout", "deadline_timeout")
    listener.close()
    selector.close()
    print(f"DONE\t{accepted}", flush=True)


def main():
    if len(sys.argv) != 3 or sys.argv[2] != "25":
        return 2
    cert = os.path.join(sys.argv[1], "certificate.pem")
    key = os.path.join(sys.argv[1], "private-key.pem")
    try:
        subprocess.run(
            ["/usr/bin/openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
             "-keyout", key, "-out", cert, "-subj", "/CN=api.anthropic.com",
             "-addext", "subjectAltName=DNS:api.anthropic.com", "-days", "1"],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            check=True, timeout=5,
        )
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(cert, key)
        serve(context)
        return 0
    except (OSError, ValueError, subprocess.SubprocessError):
        print("HELPER_ERROR", flush=True)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
