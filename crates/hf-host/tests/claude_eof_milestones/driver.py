"""Fixed four-peer fake TLS controls; output contains only closed metadata."""
import importlib.util
import os
import socket
import ssl
import sys


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


base = os.path.dirname(os.path.dirname(__file__))
reader = load("harflow_wave_driver", os.path.join(base, "claude_wave_helper", "driver.py"))
helper_script = os.path.join(base, "claude_two_tunnel", "eof_tls.py")


class MilestoneHelper(reader.Helper):
    def finish(self, expected=4):
        self.process.wait(timeout=27)
        for thread in self.readers:
            thread.join(timeout=2)
        if any(thread.is_alive() for thread in self.readers) or any(self.errors):
            raise AssertionError("capture settlement")
        while True:
            line = self.lines.get(timeout=1)
            if line is None:
                break
            self.records.append(line)
        if self.process.returncode != 0:
            raise AssertionError("helper completion")
        done = next((i for i, row in enumerate(self.records)
                     if row == "DONE\t{}".format(expected)), -1)
        if done < 0 or len(self.records) - done != expected + 1:
            raise AssertionError("milestone coverage")
        rows = self.records
        if any(not row.startswith("TLS_MILESTONE_V1\t") for row in rows[done + 1:]):
            raise AssertionError("milestone format")
        return rows


def run(root):
    helper = MilestoneHelper(helper_script, root)
    clients = []
    ports = []
    try:
        cert = os.path.join(root, "certificate.pem")
        secure, port = reader.trusted(helper.address, cert)
        clients.append(secure)
        ports.append(port)
        secure.sendall(b"x")
        secure.shutdown(socket.SHUT_WR)
        if secure.recv(1):
            raise AssertionError("unexpected server application data")
        secure.close()

        raw = socket.create_connection(helper.address, timeout=4)
        ports.append(raw.getsockname()[1])
        try:
            ssl.create_default_context().wrap_socket(raw, server_hostname="api.anthropic.com")
            raise AssertionError("untrusted certificate accepted")
        except ssl.SSLCertVerificationError:
            pass
        finally:
            raw.close()

        raw = socket.create_connection(helper.address, timeout=4)
        ports.append(raw.getsockname()[1])
        incoming, outgoing = ssl.MemoryBIO(), ssl.MemoryBIO()
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        client = context.wrap_bio(incoming, outgoing, server_side=False,
                                  server_hostname="api.anthropic.com")
        try:
            client.do_handshake()
        except ssl.SSLWantReadError:
            pass
        hello = outgoing.read()
        if not hello:
            raise AssertionError("missing generated ClientHello")
        raw.sendall(hello)
        raw.shutdown(socket.SHUT_WR)
        raw.settimeout(4)
        drained = 0
        while True:
            chunk = raw.recv(4096)
            if not chunk:
                break
            drained += len(chunk)
            if drained > 262144:
                raise AssertionError("local flight drain bound")
        raw.close()

        raw = socket.create_connection(helper.address, timeout=4)
        ports.append(raw.getsockname()[1])
        raw.shutdown(socket.SHUT_WR)
        if raw.recv(1):
            raise AssertionError("raw EOF control data")
        raw.close()

        rows = helper.finish()
        print("PEERS\t" + "\t".join(map(str, ports)))
        print("\n".join(rows))
    finally:
        for secure in clients:
            secure.close()
        helper.close()


if __name__ == "__main__":
    try:
        if len(sys.argv) != 2:
            raise AssertionError("arguments")
        run(sys.argv[1])
    except Exception:
        print("DRIVER_FAILURE", file=sys.stderr)
        raise SystemExit(1)
