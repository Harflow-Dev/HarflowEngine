import os
import select
import sys

witness = sys.argv[1]
size = 262144
stdout = bytes((index * 17 + 3) % 256 for index in range(size))
stderr = bytes((index * 29 + 7) % 256 for index in range(size))

for offset in range(0, size, 16384):
    for descriptor, payload in ((1, stdout), (2, stderr)):
        view = memoryview(payload[offset : offset + 16384])
        while view:
            written = os.write(descriptor, view)
            view = view[written:]

with open(witness + ".tmp", "wb") as completed:
    completed.write(b"complete")
os.replace(witness + ".tmp", witness)
select.select([sys.stdin.buffer], [], [], 8)
