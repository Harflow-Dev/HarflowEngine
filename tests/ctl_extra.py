"""Additional state-inspect roots: hostile journals and non-regular entries.

Every expected pair was confirmed against the reference inspect_state (HarflowMCP
0.0.11) on the same layout, except where a row's note says it deviates on purpose.
"""

import json
import os
import socket

from ctl_rows import (
    JOURNAL, LEGACY, MARKER, OPERATION, committed_by_marker, make_dirs, skip_unless_permissions_apply,
)
from ctl_support import journal, python_marker


def with_member(case, raw_member, committed=True):
    """A qualified completed journal plus one raw JSON member the codec must skip."""
    text = journal(case.state, "completed").rstrip("\n")
    data = text[:-1].encode("utf-8") + b", " + raw_member + b"}\n"
    if committed:
        committed_by_marker(case, data)
    else:
        case.write(JOURNAL, data)


def bad_escape_in_ignored_string(case):
    with_member(case, b'"note": "bad \\q escape"')


def bad_escape_in_deep_string(case):
    with_member(case, b'"note": [[{"k": ["x \\q"]}]]')


def invalid_utf8_in_ignored_string(case):
    with_member(case, b'"note": "\xff\xfe"')


def invalid_utf8_deep(case):
    with_member(case, b'"note": [[[{"k": "\xc3\x28"}]]]')


def nested_without_operation(depth):
    def build(case):
        body = b'{"operation_id": "not-an-id", "note": ' + b"[" * depth + b"]" * depth + b"}\n"
        case.write(JOURNAL, body)

    return build


def many_members_under_a_large_key(case):
    members = ", ".join(f'"m{number}": 1' for number in range(10000))
    member = ('"' + "K" * (1 << 20) + '": {' + members + "}").encode("utf-8")
    with_member(case, member)


def lock_without_permissions(case):
    skip_unless_permissions_apply(case)
    make_dirs(case)
    path = case.write(".harflow-store.lock", b"")
    os.chmod(path, 0)


def fifo_in_root(case):
    if not hasattr(os, "mkfifo"):
        case.skipTest("FIFOs exist on Unix only")
    make_dirs(case)
    os.mkfifo(os.path.join(case.state, "pipe"))


def socket_in_root(case):
    if not hasattr(socket, "AF_UNIX"):
        case.skipTest("Unix sockets are unavailable")
    make_dirs(case)
    listener = socket.socket(socket.AF_UNIX)
    case.addCleanup(listener.close)
    try:
        listener.bind(os.path.join(case.state, "sock"))
    except OSError as error:
        case.skipTest(f"socket cannot be bound: {error}")


def fifo_as_lock(case):
    if not hasattr(os, "mkfifo"):
        case.skipTest("FIFOs exist on Unix only")
    make_dirs(case)
    os.mkfifo(os.path.join(case.state, ".harflow-store.lock"))


def marker_directory_with_planned_journal(case):
    make_dirs(case, MARKER)
    case.write(JOURNAL, journal(case.state, "planned"))


def journal_symlink_with_good_pending(case):
    outside = os.path.join(case.base, "outside-journal")
    with open(outside, "w", encoding="utf-8") as handle:
        handle.write(journal(case.state, "planned"))
    make_dirs(case)
    case.symlink(outside, os.path.join(case.state, JOURNAL))
    case.write(JOURNAL + ".pending", journal(case.state, "planned"))


def lock_directory_with_planned_journal(case):
    make_dirs(case, ".harflow-store.lock")
    case.write(JOURNAL, journal(case.state, "planned"))


def legacy_directory_is_a_symlink(case):
    make_dirs(case, *LEGACY[:3])
    case.symlink(os.path.join(case.base, "elsewhere"), os.path.join(case.state, LEGACY[3]))


def non_ascii_name(case):
    case.write("café.txt", b"x")


def missing_root_equal_to_home(case):
    case.home = os.path.join(case.base, "absent-home")
    return case.home


# name, builder, status, reason, note
EXTRA_ROWS = [
    ("bad escape in an ignored string", bad_escape_in_ignored_string, "incompatible", "infrastructure_unavailable", ""),
    ("bad escape in a deep string", bad_escape_in_deep_string, "incompatible", "infrastructure_unavailable", ""),
    ("invalid utf-8 in an ignored string", invalid_utf8_in_ignored_string, "incompatible", "infrastructure_unavailable", ""),
    ("invalid utf-8 in a deep string", invalid_utf8_deep, "incompatible", "infrastructure_unavailable", ""),
    ("nested past 128, no operation id", nested_without_operation(200), "incompatible", "recovery_interrupted", ""),
    ("nested to 900, no operation id", nested_without_operation(900), "incompatible", "recovery_interrupted", ""),
    ("many members under a 1 MiB key", many_members_under_a_large_key, "incompatible", "unsupported_layout", "store"),
    ("lock with mode 000", lock_without_permissions, "ready", "compatible", ""),
    ("fifo beside an empty store", fifo_in_root, "incompatible", "unmarked_nonempty", ""),
    ("socket beside an empty store", socket_in_root, "incompatible", "unmarked_nonempty", ""),
    ("fifo as the lock", fifo_as_lock, "incompatible", "invalid_marker", ""),
    ("marker directory and planned journal", marker_directory_with_planned_journal, "incompatible", "invalid_marker", ""),
    ("journal symlink and good pending journal", journal_symlink_with_good_pending, "incompatible", "invalid_marker", ""),
    # Deliberate deviation: the reference reports infrastructure_unavailable for a lock
    # directory because its message names the lock; the plan classifies it by kind.
    ("lock directory and planned journal", lock_directory_with_planned_journal, "incompatible", "invalid_marker", "deviation"),
    ("legacy directory is a symlink", legacy_directory_is_a_symlink, "incompatible", "unmarked_nonempty", ""),
    ("non-ascii file name", non_ascii_name, "incompatible", "unmarked_nonempty", ""),
    ("missing root equal to home", missing_root_equal_to_home, "incompatible", "unsafe_root", ""),
]
