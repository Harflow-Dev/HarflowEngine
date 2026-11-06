"""Fixed, bounded syscalls for the installed-Claude no-fork preflight."""
import errno
import os
import socket
import sys
import time


def denied(call):
    try:
        call()
    except OSError as error:
        return error.errno == errno.EPERM
    return False


def fork_probe():
    child = os.fork()
    if child == 0:
        os._exit(0)
    wait_child(child)


def spawn_probe():
    child = os.posix_spawn("/usr/bin/true", ["true"], os.environ)
    wait_child(child)


def wait_child(child):
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        waited, status = os.waitpid(child, os.WNOHANG)
        if waited == child:
            if os.waitstatus_to_exitcode(status) != 0:
                raise RuntimeError("controlled child exited unsuccessfully")
            return
    raise TimeoutError("UNKNOWN: controlled child did not settle before its deadline")


def main():
    if sys.argv[1:] == ["--spawn-control"]:
        spawn_probe()
        print("spawn-control=allowed", flush=True)
        return 0
    owner_control = None
    args = sys.argv[1:]
    if args and args[0] == "--owner-control-denial":
        if len(args) != 7:
            raise ValueError("owner control sentinel arguments")
        owner_control = args[1]
        args = args[2:]
    outside, inside, allowed_port, denied_port, nonce = args[:5]
    fork_denied = denied(fork_probe)
    spawn_denied = denied(spawn_probe)
    read_denied = denied(lambda: open(outside, "rb").read(1))
    with open(inside, "rb") as canary:
        inside_ok = canary.read(1) == b"I"
    request = f"GET /{nonce}/preflight HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n".encode()
    with socket.create_connection(("127.0.0.1", int(allowed_port)), timeout=1) as stream:
        stream.sendall(request)
        allowed_ok = read_preflight_response(stream)
    denied_ok = denied(lambda: socket.create_connection(("127.0.0.1", int(denied_port)), timeout=1))
    result = f"fork={int(fork_denied)} posix_spawn={int(spawn_denied)} read={int(read_denied)} inside={int(inside_ok)} allowed={int(allowed_ok)} denied_port={int(denied_ok)}"
    if owner_control is not None:
        owner_write_denied = denied(lambda: open(owner_control, "wb").write(b"x"))
        result += f" owner_write_denied={int(owner_write_denied)}"
    print(result, flush=True)
    return 0


def read_preflight_response(stream):
    data = bytearray()
    while len(data) <= 1024 and b"\r\n\r\n" not in data:
        chunk = stream.recv(128)
        if not chunk:
            return False
        data.extend(chunk)
    header, separator, body = data.partition(b"\r\n\r\n")
    if separator != b"\r\n\r\n" or not header.startswith(b"HTTP/1.1 200 "):
        return False
    lengths = [line.split(b":", 1)[1].strip() for line in header.split(b"\r\n") if line.lower().startswith(b"content-length:")]
    if len(lengths) != 1 or not lengths[0].isdigit():
        return False
    expected = int(lengths[0])
    while len(body) < expected and len(data) <= 1024:
        chunk = stream.recv(128)
        if not chunk:
            return False
        body += chunk
        data.extend(chunk)
    return len(body) == expected and body == b"preflight-ok"


if __name__ == "__main__":
    raise SystemExit(main())
