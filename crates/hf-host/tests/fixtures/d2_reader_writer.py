import os
import sys
import time

witness = sys.argv[1]
size = 262144
stdout = bytes((index * 17 + 3) % 256 for index in range(size))
stderr = bytes((index * 29 + 7) % 256 for index in range(size))

def write_all(descriptor, payload):
    view = memoryview(payload)
    while view:
        written = os.write(descriptor, view)
        view = view[written:]


def mark(control, name):
    with open(os.path.join(control, name), "wb"):
        pass


def wait_for(control, name):
    marker = os.path.join(control, name)
    until = time.monotonic() + 5.0
    while True:
        if time.monotonic() >= until:
            raise SystemExit("producer control phase expired: " + name)
        if os.path.isfile(marker):
            return
        time.sleep(0.01)


offset_start = 0
if len(sys.argv) > 2:
    control = sys.argv[2]
    write_all(1, stdout[:1])
    write_all(2, stderr[:1])
    mark(control, "first.ready")
    wait_for(control, "next")
    write_all(1, stdout[1:2])
    write_all(2, stderr[1:2])
    mark(control, "second.ready")
    wait_for(control, "bulk")
    offset_start = 2

for offset in range(offset_start, size, 16384):
    for descriptor, payload in ((1, stdout), (2, stderr)):
        write_all(descriptor, payload[offset : offset + 16384])

with open(witness + ".tmp", "wb") as completed:
    completed.write(b"complete")
os.replace(witness + ".tmp", witness)
os.close(1)
os.close(2)
