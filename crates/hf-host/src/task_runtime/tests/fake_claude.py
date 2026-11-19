#!/usr/bin/python3 -IS
# Scripted stand-in for the agent CLI, used only by crate unit tests.
# Behaviour comes from FAKE_STEPS (comma-separated); gates are unix sockets owned by the test.
import errno
import json
import os
import socket
import subprocess
import sys


def probe_descriptors():
    for fd in range(3, 256):
        try:
            os.fstat(fd)
        except OSError as error:
            if error.errno == errno.EBADF:
                continue
            sys.exit(97)
        sys.exit(97)


def gate(name):
    link = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    link.connect(os.path.join(os.environ["FAKE_GATES"], name))
    link.recv(1)
    return link


def put(data, fd=1):
    """Writes every byte unbuffered and appends it to the test's log of what was emitted."""
    view = memoryview(data)
    while view:
        view = view[os.write(fd, view):]
    log = os.environ.get("FAKE_OUTLOG" if fd == 1 else "FAKE_ERRLOG")
    if log:
        with open(log, "ab") as handle:
            handle.write(data)


def emit(value):
    put(json.dumps(value).encode() + b"\n")


def pad_line(sid, index, size):
    message = {"id": "b%d" % index, "content": [{"type": "text", "text": "y" * size}]}
    return json.dumps({"type": "assistant", "session_id": sid, "message": message}).encode() + b"\n"


def padded(sid, total, size):
    """Whole lines of `size` characters, at least `total` bytes."""
    data, index = b"", 0
    while len(data) < total:
        data += pad_line(sid, 1000 + index, size)
        index += 1
    return data


def session():
    args = sys.argv
    return args[args.index("--session-id") + 1] if "--session-id" in args else ""


def text_line(sid, index):
    message = {"id": "m%d" % index, "content": [{"type": "text", "text": "x"}]}
    return json.dumps({"type": "assistant", "session_id": sid, "message": message}).encode() + b"\n"


def result(sid, failed):
    return {
        "type": "result",
        "subtype": "error_during_execution" if failed else "success",
        "is_error": failed,
        "session_id": sid,
        "result": "done",
    }


def descendant():
    with open(os.environ["FAKE_PIDFILE"], "w") as handle:
        handle.write(str(os.getpid()))
    gate("desc")
    sys.exit(0)


def run(steps, sid):
    sys.stdin = sys.stdin
    for step in steps:
        name, _, arg = step.partition(":")
        if name == "read":
            sys.stdin.buffer.readline()
        elif name == "init":
            emit({"type": "system", "subtype": "init", "session_id": sid, "tools": [{"name": "Read"}]})
        elif name == "text":
            put(text_line(sid, 0))
        elif name == "lines":
            put(b"".join(text_line(sid, i) for i in range(int(arg))))
        elif name == "burst":
            for i in range(int(arg)):
                put(pad_line(sid, i, 1500))
        elif name in ("mib", "flood"):
            total = int(arg) * (1 << 20) if name == "mib" else int(arg)
            data = padded(sid, total, 4000)
            for offset in range(0, len(data), 65536):
                put(data[offset : offset + 65536])
        elif name == "noread":
            import time

            while True:
                time.sleep(1)
        elif name == "result":
            emit(result(sid, False))
        elif name == "result_fail":
            emit(result(sid, True))
        elif name == "garbage":
            put(b"this is not json\n")
        elif name == "partial":
            put(b'{"type":"resu')
        elif name == "stderr":
            put(b"e" * int(arg), 2)
        elif name == "gate":
            gate(arg)
        elif name == "eofwait":
            while sys.stdin.buffer.read(4096):
                pass
        elif name == "sleep":
            import time

            time.sleep(int(arg) / 1000.0)
        elif name == "descendant":
            subprocess.Popen(
                [sys.argv[0], "--descendant"],
                stdin=subprocess.DEVNULL,
                stdout=None,
                stderr=None,
            )
        elif name == "exit":
            sys.stdout.flush()
            os._exit(int(arg))


probe_descriptors()
if "--version" in sys.argv:
    print(os.environ.get("FAKE_VERSION", "2.1.322") + " (Claude Code)")
    sys.exit(0)
if "--descendant" in sys.argv:
    descendant()
with open(os.environ["FAKE_LOG"], "a") as log:
    log.write("start\n")
run(os.environ.get("FAKE_STEPS", "read,init,text,result,eofwait,exit:0").split(","), session())
sys.exit(0)
