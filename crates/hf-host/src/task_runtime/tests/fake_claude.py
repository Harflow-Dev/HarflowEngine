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


def emit(value):
    sys.stdout.buffer.write(json.dumps(value).encode() + b"\n")
    sys.stdout.buffer.flush()


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
            sys.stdout.buffer.write(text_line(sid, 0))
            sys.stdout.buffer.flush()
        elif name == "lines":
            sys.stdout.buffer.write(b"".join(text_line(sid, i) for i in range(int(arg))))
            sys.stdout.buffer.flush()
        elif name == "result":
            emit(result(sid, False))
        elif name == "result_fail":
            emit(result(sid, True))
        elif name == "garbage":
            sys.stdout.buffer.write(b"this is not json\n")
            sys.stdout.buffer.flush()
        elif name == "partial":
            sys.stdout.buffer.write(b'{"type":"resu')
            sys.stdout.buffer.flush()
        elif name == "stderr":
            sys.stderr.write("e" * int(arg))
            sys.stderr.flush()
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
