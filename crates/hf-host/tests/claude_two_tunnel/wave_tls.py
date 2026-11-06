"""Capture-free four-total/two-active replacement-wave TLS witness."""
import importlib.util
import os
import selectors
import socket
import time

_spec = importlib.util.spec_from_file_location(
    "harflow_two_tunnel_tls", os.path.join(os.path.dirname(__file__), "fake_tls.py"))
base = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(base)


def admit(selector, sessions, listener, context, accepted):
    connection, peer = listener.accept()
    if len(sessions) >= 2:
        connection.close()
        raise ValueError("active limit")
    try:
        connection.setblocking(False)
        secure = context.wrap_socket(connection, server_side=True, do_handshake_on_connect=False)
    except BaseException:
        connection.close()
        raise
    sessions[secure] = (accepted + 1, peer, False)
    selector.register(secure, selectors.EVENT_READ)
    return accepted + 1


def serve_wave(context):
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    selector = selectors.DefaultSelector()
    sessions = {}
    accepted = 0
    try:
        listener.bind(("127.0.0.1", 0))
        listener.listen(2)
        listener.setblocking(False)
        selector.register(listener, selectors.EVENT_READ)
        deadline = time.monotonic() + 25
        print(f"READY\t{listener.getsockname()[1]}", flush=True)
        while time.monotonic() < deadline and (accepted < 4 or sessions):
            ready = selector.select(max(0, deadline - time.monotonic()))
            # Settle ready sessions before considering capacity for new admissions.
            for key, _ in ready:
                if key.fileobj is not listener:
                    base.progress(selector, sessions, key.fileobj)
            for key, _ in ready:
                if key.fileobj is listener:
                    accepted = admit(selector, sessions, listener, context, accepted)
                    if accepted == 4:
                        selector.unregister(listener)
                        listener.close()
        for secure in list(sessions):
            base.settle(selector, sessions, secure, "timeout", "deadline_timeout")
        print(f"DONE\t{accepted}", flush=True)
    finally:
        for secure in sessions:
            secure.close()
        listener.close()
        selector.close()


if __name__ == "__main__":
    raise SystemExit(base.main(serve_wave))
