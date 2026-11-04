"""Private FIFO and high volume fixtures for process race integration tests."""
import os
import select
import sys


def pressure(size):
    block = bytes(range(256))
    payload = (block * ((size + 255) // 256))[:size]
    for offset in range(0, size, 4096):
        chunk = payload[offset:offset + 4096]
        os.write(1, chunk)
        os.write(2, chunk[::-1])
    os.write(1, b"\xe2")
    os.write(2, b"\x82\xac")
    control_loop(None)


def control_loop(fifo_fd):
    stdin_fd = sys.stdin.fileno()
    while True:
        ready, _, _ = select.select([stdin_fd] + ([fifo_fd] if fifo_fd is not None else []), [], [], 3.0)
        for fd in ready:
            data = os.read(fd, 64)
            if not data or data.rstrip(b"\n") == b"quit":
                return
            if fd == fifo_fd and data.rstrip(b"\n") == b"exit":
                return


def fifo_exit(path):
    os.mkfifo(path, 0o600)
    fd = os.open(path, os.O_RDWR | os.O_NONBLOCK)
    print("READY", flush=True)
    control_loop(fd)
    os.close(fd)


def main():
    mode = sys.argv[1]
    if mode == "pressure":
        pressure(int(sys.argv[2]))
    elif mode == "fifo-exit":
        fifo_exit(sys.argv[2])
    else:
        raise SystemExit("unknown race fixture mode")


if __name__ == "__main__":
    main()
