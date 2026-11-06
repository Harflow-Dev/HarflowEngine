"""Pure closed controls for observer callback failure and payload non-access."""
import contextlib
import importlib.util
import io
import os
import sys

path = os.path.join(os.path.dirname(os.path.dirname(__file__)),
                    "claude_two_tunnel", "tls_milestones.py")
spec = importlib.util.spec_from_file_location("harflow_milestones_control", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class Poison:
    def __getattribute__(self, _name):
        raise AssertionError("payload inspected")


    def __len__(self):
        raise AssertionError("payload inspected")

    def __hash__(self):
        raise AssertionError("payload inspected")

    def __bytes__(self):
        raise AssertionError("payload inspected")

    def __str__(self):
        raise AssertionError("payload inspected")

    def __repr__(self):
        raise AssertionError("payload inspected")


class UnknownSocket:
    pass


class RegisteredSocket:
    def getpeername(self):
        return ("127.0.0.1", 43210)


def run():
    observer = module.Observer()
    secure = RegisteredSocket()
    assert observer.register(secure, 1, secure.getpeername()) is None
    assert observer.message(
        secure, "read", Poison(), module.ssl._TLSContentType.HANDSHAKE,
        module.ssl._TLSMessageType.CLIENT_HELLO, Poison()
    ) is None
    if observer.failed or observer.records[secure][2] != module.READ_CLIENT_HELLO:
        raise AssertionError("native ClientHello callback")

    idle = module.Observer()
    idle_socket = RegisteredSocket()
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        assert idle.register(idle_socket, 1, idle_socket.getpeername()) is None
        idle.emit()
    if output.getvalue() != "TLS_MILESTONE_V1\t1\t127.0.0.1\t43210\t0\tunseen\n":
        raise AssertionError("empty admission milestone")

    other = module.Observer()
    other_socket = RegisteredSocket()
    other.register(other_socket, 1, other_socket.getpeername())
    assert other.sni(other_socket, "unapproved.example", None) is None
    assert other.records[other_socket][3] == "other"
    ensure_closed(observer, lambda: observer.message(
        UnknownSocket(), Poison(), Poison(), Poison(), Poison(), Poison()))

    direction = module.Observer()
    direction_socket = RegisteredSocket()
    direction.register(direction_socket, 1, direction_socket.getpeername())
    ensure_closed(direction, lambda: direction.message(
        direction_socket, Poison(), Poison(), Poison(), Poison(), Poison()))

    message = module.Observer()
    message_socket = RegisteredSocket()
    message.register(message_socket, 1, message_socket.getpeername())
    ensure_closed(message, lambda: message.message(
        message_socket, "read", Poison(), object(), object(), Poison()))

    overflow = module.Observer()
    for ordinal in range(1, 6):
        candidate = RegisteredSocket()
        candidate.getpeername = lambda port=43210 + ordinal: ("127.0.0.1", port)
        overflow.register(candidate, ordinal, candidate.getpeername())
    ensure_closed(overflow, lambda: None)


def ensure_closed(observer, action):
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        assert action() is None
        observer.emit()
    if output.getvalue() != "MILESTONE_ERROR\n":
        raise AssertionError("observer failure was not closed")


if __name__ == "__main__":
    try:
        run()
        print("OBSERVER_CONTROLS\tpass")
    except Exception:
        print("OBSERVER_CONTROL_FAILURE", file=sys.stderr)
        raise SystemExit(1)
