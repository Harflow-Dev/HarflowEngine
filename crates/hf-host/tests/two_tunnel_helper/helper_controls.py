"""Exercise the production two-session helper with offline loopback controls."""
import os
import queue
import socket
import ssl
import subprocess
import sys
import tempfile
import threading


def read_lines(stream, lines):
    try:
        for raw in iter(stream.readline, b""):
            lines.put(raw.decode("ascii", errors="strict").rstrip("\n"))
    finally:
        lines.put(None)


def drain_quietly(stream):
    for _ in iter(lambda: stream.read(4096), b""):
        pass


class Helper:
    def __init__(self, script, directory):
        self.process = subprocess.Popen(
            [sys.executable, "-I", "-S", script, directory, "25"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env={
                "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
                "LANG": "C",
                "LC_ALL": "C",
                "PYTHONNOUSERSITE": "1",
                "PYTHONDONTWRITEBYTECODE": "1",
            },
        )
        self.lines = queue.Queue()
        self.reader = threading.Thread(
            target=read_lines, args=(self.process.stdout, self.lines), daemon=True
        )
        self.stderr = threading.Thread(
            target=drain_quietly, args=(self.process.stderr,), daemon=True
        )
        self.reader.start()
        self.stderr.start()
        ready = self.next_line(8)
        fields = ready.split("\t") if ready else []
        if len(fields) != 2 or fields[0] != "READY" or not fields[1].isdigit():
            self.stop()
            raise AssertionError("helper readiness missing")
        self.port = int(fields[1])

    def __enter__(self):
        return self

    def __exit__(self, _kind, _value, _traceback):
        if self.process.poll() is None:
            self.stop()

    def next_line(self, timeout):
        try:
            return self.lines.get(timeout=timeout)
        except queue.Empty as error:
            raise AssertionError("helper event deadline") from error

    def session(self, timeout):
        line = self.next_line(timeout)
        fields = line.split("\t") if line else []
        if len(fields) != 8 or fields[0] != "SESSION":
            raise AssertionError("helper session record missing")
        ordinal, address, peer, certificate, application, status, closed = fields[1:]
        if address != "127.0.0.1" or not peer.isdigit():
            raise AssertionError("helper peer identity invalid")
        return {
            "ordinal": int(ordinal),
            "peer": int(peer),
            "certificate": certificate == "1",
            "application": application == "1",
            "status": status,
            "closed": closed == "1",
        }

    def finish(self):
        try:
            code = self.process.wait(timeout=27)
        except subprocess.TimeoutExpired as error:
            self.stop()
            raise AssertionError("helper exceeded fixed lifetime") from error
        self.reader.join(timeout=2)
        self.stderr.join(timeout=2)
        if self.reader.is_alive() or self.stderr.is_alive():
            raise AssertionError("helper output readers did not settle")
        done = self.next_line(1)
        if code != 0 or done != "DONE\t2":
            raise AssertionError("helper did not settle two sessions")

    def controller_stop(self):
        self.process.terminate()
        code = self.process.wait(timeout=5)
        self.reader.join(timeout=2)
        self.stderr.join(timeout=2)
        if self.reader.is_alive() or self.stderr.is_alive():
            raise AssertionError("controller-stopped helper readers did not settle")
        records = []
        while True:
            try:
                line = self.lines.get_nowait()
            except queue.Empty:
                break
            if line is not None:
                records.append(line)
        return code, records

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
            self.process.wait()
        if self.process.stdout:
            self.process.stdout.close()
        if self.process.stderr:
            self.process.stderr.close()
        self.reader.join(timeout=2)
        self.stderr.join(timeout=2)


def run_client(address, ca_file, mode, outcomes):
    raw = socket.create_connection(address, timeout=5)
    local_port = raw.getsockname()[1]
    try:
        if mode == "trusted":
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
            context.check_hostname = True
            context.verify_mode = ssl.CERT_REQUIRED
            context.load_verify_locations(cafile=ca_file)
            secure = context.wrap_socket(raw, server_hostname="api.anthropic.com")
            secure.sendall(b"x")
            secure.shutdown(socket.SHUT_WR)
            while secure.recv(1):
                pass
            secure.close()
            outcomes[mode] = (local_port, "application_data")
        else:
            context = ssl.create_default_context()
            try:
                context.wrap_socket(raw, server_hostname="api.anthropic.com")
                outcomes[mode] = (local_port, "unexpected_success")
            except ssl.SSLCertVerificationError:
                outcomes[mode] = (local_port, "verify_rejected")
    except (OSError, ssl.SSLError):
        outcomes[mode] = (local_port, "client_io_failure")
    finally:
        raw.close()


def positive_and_rejection(helper_script):
    with tempfile.TemporaryDirectory(prefix="hf-two-tunnel-controls-") as directory:
        with Helper(helper_script, directory) as helper:
            outcomes = {}
            clients = [
                threading.Thread(
                    target=run_client,
                    args=(("127.0.0.1", helper.port), os.path.join(directory, "certificate.pem"), mode, outcomes),
                )
                for mode in ("trusted", "default")
            ]
            for client in clients:
                client.start()
            for client in clients:
                client.join(timeout=7)
            if any(client.is_alive() for client in clients):
                raise AssertionError("TLS controller clients did not settle")
            sessions = [helper.session(3), helper.session(3)]
            helper.finish()
            if outcomes.get("trusted", (0, ""))[1] != "application_data":
                raise AssertionError("trusted TLS application-data control failed")
            if outcomes.get("default", (0, ""))[1] != "verify_rejected":
                raise AssertionError("default certificate rejection control failed")
            if {item[0] for item in outcomes.values()} != {item["peer"] for item in sessions}:
                raise AssertionError("controller and helper peer identities differ")
            if len({item["peer"] for item in sessions}) != 2:
                raise AssertionError("helper peer identities are not unique")
            by_peer = {item["peer"]: item for item in sessions}
            trusted = by_peer[outcomes["trusted"][0]]
            rejected = by_peer[outcomes["default"][0]]
            if not trusted["application"] or trusted["certificate"] or trusted["status"] != "application_data":
                raise AssertionError("trusted session was not detected as application data")
            if not rejected["certificate"] or rejected["application"] or rejected["status"] != "cert_alert":
                raise AssertionError("default client produced no explicit certificate alert")
            if not all(item["closed"] for item in sessions):
                raise AssertionError("positive control sessions did not close")


def eof_and_idle_timeout(helper_script):
    with tempfile.TemporaryDirectory(prefix="hf-two-tunnel-controls-") as directory:
        with Helper(helper_script, directory) as helper:
            eof = socket.create_connection(("127.0.0.1", helper.port), timeout=5)
            eof.shutdown(socket.SHUT_WR)
            eof.close()
            eof_session = helper.session(5)
            if eof_session["certificate"] or eof_session["status"] == "cert_alert":
                raise AssertionError("raw TCP EOF became certificate rejection")
            idle = socket.create_connection(("127.0.0.1", helper.port), timeout=5)
            idle_session = helper.session(27)
            idle.close()
            helper.finish()
            if idle_session["certificate"] or idle_session["status"] != "timeout":
                raise AssertionError("idle timeout became certificate rejection")
            if not eof_session["closed"] or not idle_session["closed"]:
                raise AssertionError("EOF or timeout session did not close")
            if eof_session["peer"] == idle_session["peer"]:
                raise AssertionError("EOF and idle peer identities were reused")


def trusted_handshake_only(address, ca_file):
    raw = socket.create_connection(address, timeout=5)
    peer = raw.getsockname()[1]
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.check_hostname = True
    context.verify_mode = ssl.CERT_REQUIRED
    context.load_verify_locations(cafile=ca_file)
    secure = context.wrap_socket(raw, server_hostname="api.anthropic.com")
    return secure, peer


def controller_stop_incomplete_session(helper_script):
    with tempfile.TemporaryDirectory(prefix="hf-two-tunnel-controls-") as directory:
        with Helper(helper_script, directory) as helper:
            outcomes = {}
            run_client(
                ("127.0.0.1", helper.port),
                os.path.join(directory, "certificate.pem"),
                "trusted",
                outcomes,
            )
            first = helper.session(5)
            if (
                outcomes.get("trusted", (0, ""))[1] != "application_data"
                or first["status"] != "application_data"
                or not first["application"]
                or first["certificate"]
            ):
                raise AssertionError("first trusted session did not settle positively")
            secure, second_peer = trusted_handshake_only(
                ("127.0.0.1", helper.port),
                os.path.join(directory, "certificate.pem"),
            )
            try:
                code, records = helper.controller_stop()
            finally:
                secure.close()
            second_records = [
                line.split("\t")
                for line in records
                if line.startswith("SESSION\t")
                and len(line.split("\t")) == 8
                and line.split("\t")[3] == str(second_peer)
            ]
            if any(record[4] == "1" or record[6] == "cert_alert" for record in second_records):
                raise AssertionError("controller stop became certificate rejection")
            if any(record[5] == "1" for record in second_records):
                raise AssertionError("controller stop session reported application data")
            if code == 0 or "DONE\t2" in records:
                raise AssertionError("controller stop produced a complete helper verdict")
            if second_peer == first["peer"]:
                raise AssertionError("controller-stop peer identity was reused")


def main():
    if len(sys.argv) != 2:
        return 2
    helper_script = os.path.abspath(sys.argv[1])
    try:
        positive_and_rejection(helper_script)
        print("CONTROL\ttrusted_application_data\tpass")
        print("CONTROL\tdefault_certificate_rejection\tpass")
        print("CONTROL\tpeer_identity_binding\tpass")
        controller_stop_incomplete_session(helper_script)
        print("CONTROL\tcontroller_stop_not_certificate_rejection\tpass")
        eof_and_idle_timeout(helper_script)
        print("CONTROL\traw_tcp_eof_not_certificate_rejection\tpass")
        print("CONTROL\tidle_timeout_not_certificate_rejection\tpass")
    except Exception:
        print("CONTROL\tcontrols\tfail", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
