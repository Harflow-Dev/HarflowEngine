"""Capture-free two-session loopback TLS witness; fixed readiness lifetime."""
import os
import selectors
import socket
import ssl
import subprocess
import sys
import time

CERT_ALERTS = {
    "TLSV1_ALERT_UNKNOWN_CA", "SSLV3_ALERT_CERTIFICATE_UNKNOWN",
    "TLSV1_ALERT_BAD_CERTIFICATE", "SSLV3_ALERT_BAD_CERTIFICATE",
    "TLSV1_ALERT_CERTIFICATE_UNKNOWN",
}


def settle(selector, sessions, secure, status, certificate=False, application=False):
    ordinal, peer, _ = sessions.pop(secure)
    selector.unregister(secure)
    secure.close()
    print(f"SESSION\t{ordinal}\t{peer[0]}\t{peer[1]}\t{int(certificate)}\t{int(application)}\t{status}\t1", flush=True)


def progress(selector, sessions, secure):
    ordinal, peer, handshake = sessions[secure]
    try:
        if not handshake:
            secure.do_handshake()
            sessions[secure] = (ordinal, peer, True)
        data_present = bool(secure.recv(1))
        settle(selector, sessions, secure, "application_data" if data_present else "eof",
               application=data_present)
    except ssl.SSLWantReadError:
        selector.modify(secure, selectors.EVENT_READ)
    except ssl.SSLWantWriteError:
        selector.modify(secure, selectors.EVENT_WRITE)
    except ssl.SSLError as error:
        certificate = error.reason in CERT_ALERTS
        settle(selector, sessions, secure, "cert_alert" if certificate else "tls_error", certificate)
    except OSError:
        settle(selector, sessions, secure, "io_error")


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
        settle(selector, sessions, secure, "timeout")
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
