"""Bounded local TLS client pair used against direct or fixed CONNECT routes."""
import socket
import ssl
import sys


def route(mode, port):
    raw = socket.create_connection(("127.0.0.1", port), timeout=3)
    raw.settimeout(5)
    if mode == "proxy":
        raw.sendall(
            b"CONNECT api.anthropic.com:443 HTTP/1.1\r\n"
            b"Host: api.anthropic.com:443\r\n\r\n"
        )
        response = bytearray()
        while not response.endswith(b"\r\n\r\n"):
            part = raw.recv(1)
            if not part:
                raise AssertionError("CONNECT closed before headers")
            response.extend(part)
            if len(response) > 4096:
                raise AssertionError("CONNECT response exceeded bound")
        if not response.startswith(b"HTTP/1.1 200 Connection Established"):
            raise AssertionError("CONNECT response category")
    return raw


def trusted(mode, port, ca_file):
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.maximum_version = ssl.TLSVersion.TLSv1_2
    context.set_alpn_protocols(["h2", "http/1.1"])
    context.check_hostname = True
    context.verify_mode = ssl.CERT_REQUIRED
    context.load_verify_locations(cafile=ca_file)
    raw = route(mode, port)
    local_port = raw.getsockname()[1]
    secure = context.wrap_socket(raw, server_hostname="api.anthropic.com")
    cert = secure.getpeercert()
    san = tuple(name for kind, name in cert.get("subjectAltName", ()) if kind == "DNS")
    if (secure.version() != "TLSv1.2" or secure.cipher() is None
            or secure.selected_alpn_protocol() is not None
            or "api.anthropic.com" not in san
            or cert.get("subject") != cert.get("issuer")):
        raise AssertionError("trusted TLS metadata mismatch")
    secure.sendall(b"x")
    secure.shutdown(socket.SHUT_WR)
    while secure.recv(1):
        pass
    secure.close()
    print(f"TRUSTED\tapplication_data\tTLSv1.2\tALPN-none\tSAN-self-issued\t{local_port}", flush=True)


def default_rejection(mode, port):
    context = ssl.create_default_context()
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.maximum_version = ssl.TLSVersion.TLSv1_2
    context.set_alpn_protocols(["h2", "http/1.1"])
    try:
        raw = route(mode, port)
        local_port = raw.getsockname()[1]
        secure = context.wrap_socket(raw, server_hostname="api.anthropic.com")
        secure.close()
    except ssl.SSLCertVerificationError:
        print(f"DEFAULT\tcertificate-verification-failed\t{local_port}", flush=True)
        return
    raise AssertionError("default verification accepted generated certificate")


def main():
    if len(sys.argv) != 4 or sys.argv[1] not in {"direct", "proxy"}:
        raise SystemExit(2)
    selected_mode = sys.argv[1]
    selected_port = int(sys.argv[2])
    trusted(selected_mode, selected_port, sys.argv[3])
    if selected_mode == "direct":
        default_rejection(selected_mode, selected_port)
    else:
        trusted(selected_mode, selected_port, sys.argv[3])


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("LOCAL_CONTROL_FAILED", file=sys.stderr, flush=True)
        raise SystemExit(1)
