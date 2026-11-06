"""Pinned local peers; only bounded helper metadata and peer identities escape."""
import contextlib
import io
import os
import queue
import socket
import ssl
import subprocess
import sys
import threading


class Helper:
    def __init__(self, script, root):
        self.process = subprocess.Popen(
            [sys.executable, "-I", "-S", script, root, "25"],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": root,
                 "LANG": "C", "LC_ALL": "C", "PYTHONDONTWRITEBYTECODE": "1"})
        self.lines = queue.Queue(maxsize=16)
        self.errors = [False, False]
        self.readers = []
        try:
            for index, stream in enumerate((self.process.stdout, self.process.stderr)):
                reader = threading.Thread(target=self.drain, args=(stream, index))
                reader.start()
                self.readers.append(reader)
            self.records = [self.line()]
            parts = self.records[0].split("\t")
            if len(parts) != 2 or parts[0] != "READY":
                raise AssertionError("readiness")
            self.address = ("127.0.0.1", int(parts[1]))
        except BaseException:
            self.close()
            raise

    def drain(self, stream, index):
        count = 0
        try:
            for raw in iter(lambda: stream.readline(513), b""):
                count += len(raw)
                if count > 4096 or len(raw) > 512 or index == 1:
                    self.errors[index] = True
                    continue
                try:
                    self.lines.put_nowait(raw.decode("ascii").rstrip("\n"))
                except (queue.Full, UnicodeError):
                    self.errors[index] = True
        except Exception:
            self.errors[index] = True
        finally:
            if index == 0:
                try:
                    self.lines.put_nowait(None)
                except queue.Full:
                    self.errors[index] = True

    def line(self):
        value = self.lines.get(timeout=5)
        if value is None:
            raise AssertionError("early EOF")
        return value

    def finish(self, expected=4):
        self.process.wait(timeout=27)
        for reader in self.readers:
            reader.join(timeout=2)
        if any(reader.is_alive() for reader in self.readers) or any(self.errors):
            raise AssertionError("capture settlement")
        while True:
            line = self.lines.get(timeout=1)
            if line is None:
                break
            self.records.append(line)
        if self.process.returncode != 0 or self.records[-1] != "DONE\t{}".format(expected):
            raise AssertionError("completion")
        return self.records

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=3)
        for reader in self.readers:
            reader.join(timeout=2)
        if any(reader.is_alive() for reader in self.readers):
            raise AssertionError("reader custody")
        self.process.stdout.close()
        self.process.stderr.close()


def trusted(address, certificate):
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.check_hostname = True
    context.verify_mode = ssl.CERT_REQUIRED
    context.load_verify_locations(cafile=certificate)
    raw = socket.create_connection(address, timeout=4)
    port = raw.getsockname()[1]
    return context.wrap_socket(raw, server_hostname="api.anthropic.com"), port


def run(script, root):
    helper = Helper(script, root)
    clients = []
    try:
        certificate = os.path.join(root, "certificate.pem")
        first, first_port = trusted(helper.address, certificate)
        clients.append(first)
        second, second_port = trusted(helper.address, certificate)
        clients.append(second)
        # Both verified handshakes finish before either sends application data.
        ports = [first_port, second_port]
        for secure in clients:
            secure.sendall(b"x")
            secure.shutdown(socket.SHUT_WR)
            if secure.recv(1):
                raise AssertionError("unexpected server application data")
            secure.close()
        raw = socket.create_connection(helper.address, timeout=4)
        ports.append(raw.getsockname()[1])
        try:
            ssl.create_default_context().wrap_socket(raw, server_hostname="api.anthropic.com")
            raise AssertionError("untrusted accepted")
        except ssl.SSLCertVerificationError:
            pass
        finally:
            raw.close()
        with socket.create_connection(helper.address, timeout=4) as raw:
            ports.append(raw.getsockname()[1])
            raw.shutdown(socket.SHUT_WR)
            if raw.recv(1):
                raise AssertionError("raw EOF control data")
        records = helper.finish()
        print("PEERS\t" + "\t".join(map(str, ports)))
        print("OVERLAP\tverified-two-before-data")
        print("\n".join(records))
    finally:
        for secure in clients:
            secure.close()
        helper.close()


def verify_drain_failure():
    class FailedStream:
        def readline(self, _limit):
            raise OSError("private-error-marker")
    helper = Helper.__new__(Helper)
    helper.lines = queue.Queue(maxsize=16)
    helper.errors = [False, False]
    stderr = io.StringIO()
    with contextlib.redirect_stderr(stderr):
        reader = threading.Thread(target=helper.drain, args=(FailedStream(), 0))
        reader.start()
        reader.join(timeout=2)
    if reader.is_alive() or not helper.errors[0] or stderr.getvalue():
        raise AssertionError("closed reader failure")
    if helper.lines.get_nowait() is not None:
        raise AssertionError("reader completion")


if __name__ == "__main__":
    try:
        verify_drain_failure()
        if len(sys.argv) == 4 and sys.argv[3] == "partial":
            helper = Helper(sys.argv[1], sys.argv[2])
            try:
                with socket.create_connection(helper.address, timeout=4) as raw:
                    port = raw.getsockname()[1]
                    raw.shutdown(socket.SHUT_WR)
                    if raw.recv(1):
                        raise AssertionError("partial EOF")
                records = helper.finish(1)
                print("PEER\t{}".format(port))
                print("\n".join(records))
            finally:
                helper.close()
        elif len(sys.argv) == 3:
            run(sys.argv[1], sys.argv[2])
        else:
            raise AssertionError("arguments")
    except Exception:
        print("DRIVER_FAILURE", file=sys.stderr)
        raise SystemExit(1)
