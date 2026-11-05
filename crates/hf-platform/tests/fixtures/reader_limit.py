import os
import select
import sys

remaining = 8 * 1024 * 1024 + 16384
block = bytes(range(256)) * 64
while remaining:
    payload = block[: min(len(block), remaining)]
    for descriptor in (1, 2):
        view = memoryview(payload)
        while view:
            written = os.write(descriptor, view)
            view = view[written:]
    remaining -= len(payload)
select.select([sys.stdin.buffer], [], [], 8)
