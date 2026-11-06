"""Owned untrusted TLS endpoint for the live-auth negative control only."""
import json
import os
import selectors
import socket
import ssl
import subprocess
import sys
import time


def main():
    if len(sys.argv) != 3:
        return 2
    directory = sys.argv[1]
    timeout = int(sys.argv[2])
    if not 1 <= timeout <= 25:
        return 2
    cert = os.path.join(directory, "certificate.pem")
    key = os.path.join(directory, "private-key.pem")
    subprocess.run(
        ["/usr/bin/openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
         "-keyout", key, "-out", cert, "-subj", "/CN=api.anthropic.com",
         "-addext", "subjectAltName=DNS:api.anthropic.com", "-days", "1"],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        check=True, timeout=5,
    )
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert, key)
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 0)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    listener.setblocking(False)
    print(json.dumps({"ready": True, "port": listener.getsockname()[1]}), flush=True)
    deadline = time.monotonic() + timeout
    selector = selectors.DefaultSelector()
    selector.register(listener, selectors.EVENT_READ)
    events = selector.select(max(0, deadline - time.monotonic()))
    if not events:
        print('{"accepted":false,"certificate_rejected":false,"application_data":false}', flush=True)
        return 0
    connection, _ = listener.accept()
    listener.close()
    connection.settimeout(max(0.1, deadline - time.monotonic()))
    received = False
    certificate_alert = False
    try:
        with context.wrap_socket(connection, server_side=True) as secure:
            secure.settimeout(max(0.1, deadline - time.monotonic()))
            received = bool(secure.recv(1))
    except ssl.SSLError as error:
        certificate_alert = error.reason in {
            "TLSV1_ALERT_UNKNOWN_CA", "SSLV3_ALERT_CERTIFICATE_UNKNOWN",
            "TLSV1_ALERT_BAD_CERTIFICATE", "SSLV3_ALERT_BAD_CERTIFICATE",
            "TLSV1_ALERT_CERTIFICATE_UNKNOWN",
        }
    except (OSError, TimeoutError):
        pass
    print(json.dumps({"accepted": True, "certificate_rejected": certificate_alert,
                      "application_data": received}), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
