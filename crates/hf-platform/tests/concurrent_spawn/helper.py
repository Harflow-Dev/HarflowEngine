import fcntl
import os
import select
import sys
import time


def probe(mode, path):
    fd = os.open(path, os.O_RDWR)
    try:
        try:
            lock_mode = fcntl.LOCK_SH if mode == "shared" else fcntl.LOCK_EX
            fcntl.lockf(fd, lock_mode | fcntl.LOCK_NB)
        except BlockingIOError:
            print("blocked", flush=True)
        else:
            print("free", flush=True)
            fcntl.lockf(fd, fcntl.LOCK_UN)
    finally:
        os.close(fd)


def hold():
    print("ready", flush=True)
    sys.stdin.readline()


def inventory(path):
    if any(key.startswith("HARFLOW_R04C_") for key in os.environ):
        raise RuntimeError("test helper environment leaked into spawned process")
    with os.scandir("/dev/fd") as entries:
        candidates = [int(entry.name) for entry in entries if entry.name.isdigit()]
    open_fds = []
    for candidate in candidates:
        if candidate < 3:
            continue
        try:
            os.fstat(candidate)
        except OSError:
            continue
        open_fds.append(str(candidate))
    rendered = " ".join(open_fds)
    if len(rendered) > 4096:
        raise RuntimeError("descriptor inventory exceeded 4 KiB")
    with open(path, "w", encoding="ascii") as output:
        output.write(rendered)


def monitor(path):
    fd = os.open(path, os.O_RDWR)
    os.set_blocking(0, False)
    with open(path + ".ready", "w", encoding="ascii") as ready:
        ready.write("ready\n")
    attempts = acquired = 0
    end = time.monotonic() + 5
    reason = ""
    while True:
        if attempts >= 5_000_000:
            reason = "attempt_cap"
            break
        if time.monotonic() >= end:
            reason = "timeout"
            break
        attempts += 1
        try:
            fcntl.lockf(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            pass
        else:
            acquired += 1
            fcntl.lockf(fd, fcntl.LOCK_UN)
        if select.select([0], [], [], 0)[0]:
            command = os.read(0, 64)
            if not command:
                reason = "eof"
                break
            if b"stop" in command:
                reason = "stop"
                break
    print(attempts, acquired, reason, flush=True)
    os.close(fd)


if __name__ == "__main__":
    mode = sys.argv[1]
    if mode == "probe":
        probe(sys.argv[2], sys.argv[3])
    elif mode == "hold":
        hold()
    elif mode == "monitor":
        monitor(sys.argv[2])
    elif mode == "inventory":
        inventory(sys.argv[2])
    else:
        raise SystemExit("unknown helper mode")
