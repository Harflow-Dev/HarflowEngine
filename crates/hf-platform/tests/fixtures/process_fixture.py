"""Private cooperative helpers for isolated hf-platform process tests."""
import ctypes
import json
import os
import signal
import subprocess
import sys


class BSDInfo(ctypes.Structure):
    _fields_ = [
        ("flags", ctypes.c_uint32), ("status", ctypes.c_uint32),
        ("xstatus", ctypes.c_uint32), ("pid", ctypes.c_uint32),
        ("ppid", ctypes.c_uint32), ("uid", ctypes.c_uint32),
        ("gid", ctypes.c_uint32), ("ruid", ctypes.c_uint32),
        ("rgid", ctypes.c_uint32), ("svuid", ctypes.c_uint32),
        ("svgid", ctypes.c_uint32), ("rfu", ctypes.c_uint32),
        ("comm", ctypes.c_char * 16), ("name", ctypes.c_char * 32),
        ("nfiles", ctypes.c_uint32), ("pgid", ctypes.c_uint32),
        ("pjobc", ctypes.c_uint32), ("tdev", ctypes.c_uint32),
        ("tpgid", ctypes.c_uint32), ("nice", ctypes.c_int32),
        ("start_sec", ctypes.c_uint64), ("start_usec", ctypes.c_uint64),
    ]


def probe(pid):
    library = ctypes.CDLL("/usr/lib/libproc.dylib")
    call = library.proc_pidinfo
    call.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
    call.restype = ctypes.c_int
    info = BSDInfo()
    size = call(pid, 3, 0, ctypes.byref(info), ctypes.sizeof(info))
    if size != ctypes.sizeof(info) or info.pid != pid:
        raise RuntimeError("incomplete independent proc_pidinfo result: size={} expected={} pid={} observed={}".format(
            size, ctypes.sizeof(info), pid, info.pid))
    print(info.pid, info.start_sec, info.start_usec, info.status, size)


def hold():
    print("READY", flush=True)
    for line in sys.stdin.buffer:
        command = line.rstrip(b"\n")
        if command == b"quit":
            print("BYE", flush=True)
            return
        if command.startswith(b"ping "):
            os.write(1, b"PONG " + command[5:] + b"\n")


def pressure(size):
    block = bytes(range(256))
    payload = (block * ((size + 255) // 256))[:size]
    for offset in range(0, size, 4096):
        chunk = payload[offset:offset + 4096]
        os.write(1, chunk)
        os.write(2, chunk[::-1])
    os.write(1, b"\xe2")
    os.write(2, b"\x82\xac")
    hold()


def partial_utf8():
    os.write(1, b"\xe2")
    if sys.stdin.buffer.readline().rstrip(b"\n") == b"continue":
        os.write(1, b"\x82\xac")
    hold()


def pipeholder():
    # Descendant deliberately inherits the parent's private stdio channels.
    child = subprocess.Popen([sys.executable, __file__, "pipehold"], close_fds=True, start_new_session=True)
    print("PIPEHOLDER {} {}".format(child.pid, os.getpgid(child.pid)), flush=True)


def pipehold(fd):
    sys.stdin.buffer.readline()


def main():
    mode = sys.argv[1]
    if mode == "probe":
        probe(int(sys.argv[2]))
    elif mode == "group":
        print(os.getpgid(int(sys.argv[2])))
    elif mode == "hold":
        hold()
    elif mode == "pressure":
        pressure(int(sys.argv[2]))
    elif mode == "ignore-term":
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        hold()
    elif mode == "exit":
        sys.exit(int(sys.argv[2]))
    elif mode == "partial":
        partial_utf8()
    elif mode == "marker":
        with open(sys.argv[2], "w", encoding="ascii") as marker:
            marker.write("started")
        hold()
    elif mode == "pipeholder":
        pipeholder()
    elif mode == "pipehold":
        pipehold(None)
    else:
        raise SystemExit("unknown fixture mode")


if __name__ == "__main__":
    main()
