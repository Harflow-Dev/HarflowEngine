"""The state-inspect classification table: one builder per root state.

Each builder prepares `case.state` (or another root it returns) and the expected
status and reason follow plan decision P5. Python's marker layout is the oracle for
the markers and journals; no byte of it comes from the Rust codec.
"""

import os

from ctl_support import (
    APPLICATION_ID, database, identity_text, journal, python_marker, rust_root, storage_constants,
)

LEGACY = ("workflows", "workflow-runs", "workflow-leases", "workflow-owners")
OPERATION = "0123456789abcdef0123456789abcdef"
MARKER = ".harflow-store.json"
JOURNAL = ".harflow-recovery.json"


def make_dirs(case, *names):
    os.makedirs(case.state, exist_ok=True)
    for name in names:
        os.makedirs(os.path.join(case.state, name), exist_ok=True)


def missing(case):
    return None


def missing_under_symlink(case):
    target = os.path.join(case.base, "real")
    os.mkdir(target)
    link = os.path.join(case.base, "link")
    case.symlink(target, link)
    return os.path.join(link, "state")


def empty(case):
    make_dirs(case)


def lock_only(case):
    case.write(".harflow-store.lock", b"")


def pending_marker(case):
    case.write(".harflow-store.json.pending", b"{}")


def legacy_dirs(case):
    make_dirs(case, *LEGACY)


def legacy_dirs_with_lock(case):
    make_dirs(case, *LEGACY)
    case.write(".harflow-store.lock", b"")


def legacy_dir_with_content(case):
    make_dirs(case, *LEGACY)
    case.write("workflows/item", b"x")


def stray_file(case):
    case.write("stray.txt", b"x")


def stray_directory(case):
    make_dirs(case, "tasks")


def lock_is_a_directory(case):
    make_dirs(case, ".harflow-store.lock")


def many_entries(case):
    make_dirs(case)
    for number in range(300):
        case.write(f"entry-{number}", b"")


def python_store(case):
    case.write(MARKER, python_marker())


def foreign_product(case):
    case.write(MARKER, python_marker(product="elsewhere"))


def foreign_format(case):
    case.write(MARKER, python_marker(format="harflow.task"))


def invalid_json_marker(case):
    case.write(MARKER, b"{not json")


def array_marker(case):
    case.write(MARKER, b"[1, 2]\n")


def binary_marker(case):
    case.write(MARKER, b"\xff\xfe\x00{")


def marker_is_a_directory(case):
    make_dirs(case, MARKER)


def oversize_marker(case):
    body = python_marker().rstrip("\n")
    case.write(MARKER, body + " " * (16385 - len(body)))


def largest_marker(case):
    body = python_marker().rstrip("\n")
    case.write(MARKER, body + " " * (16384 - len(body)))


def rust_current(case):
    rust_root(case.state)


def rust_future_identity(case):
    rust_root(case.state, identity_version=storage_constants() + 1)


def rust_older_identity(case):
    rust_root(case.state, identity_version=storage_constants() - 1)


def rust_malformed_identity(case):
    rust_root(case.state)
    case.write("store.identity", identity_text(storage_constants()).replace("\n", "\r\n"))


def rust_missing_database(case):
    rust_root(case.state)
    os.remove(os.path.join(case.state, "store.sqlite3"))


def rust_foreign_database(case):
    rust_root(case.state, application_id=APPLICATION_ID + 1)


def rust_future_database(case):
    rust_root(case.state, header_version=storage_constants() + 1)


def rust_not_a_database(case):
    rust_root(case.state)
    case.write("store.sqlite3", b"x" * 200)


def rust_short_database(case):
    rust_root(case.state)
    case.write("store.sqlite3", b"SQLite format 3\x00")


def rust_with_other_generation_database(case):
    rust_root(case.state, generation="a" * 32)
    os.remove(os.path.join(case.state, "store.sqlite3"))
    os.remove(os.path.join(case.state, "store.sqlite3-wal"))
    database(os.path.join(case.state, "store.sqlite3"), storage_constants(), generation="c" * 32)


def journal_with_marker(case, phase, marker_operation):
    marker = python_marker(**({"recovery_operation_id": marker_operation} if marker_operation else {}))
    case.write(MARKER, marker)
    case.write(JOURNAL, journal(case.state, phase))


def interrupted_planned(case):
    case.write(JOURNAL, journal(case.state, "planned"))


def interrupted_prepared(case):
    case.write(JOURNAL, journal(case.state, "prepared"))


def interrupted_backed_up(case):
    case.write(JOURNAL, journal(case.state, "backed_up"))


def interrupted_committed_without_marker(case):
    journal_with_marker(case, "committed", None)


def interrupted_committed_other_marker(case):
    journal_with_marker(case, "committed", "f" * 32)


def interrupted_garbage(case):
    case.write(JOURNAL, b"\x00garbage")


def interrupted_other_root(case):
    case.write(JOURNAL, journal(case.state + "-other", "completed"))


def interrupted_bad_operation(case):
    case.write(JOURNAL, journal(case.state, "completed", operation="NOT-HEX", proposal={}))


def interrupted_bad_backup_path(case):
    case.write(JOURNAL, journal(case.state, "completed", backup_path="/elsewhere"))


JOURNAL_BOUND = 16 * 1024 * 1024


def padded_journal(case, size, **overrides):
    text = journal(case.state, "completed", **overrides)
    assert len(text) <= size
    return text + " " * (size - len(text))


def interrupted_oversize(case):
    case.write(JOURNAL, padded_journal(case, JOURNAL_BOUND + 1))


def journal_at_the_bound(case):
    case.write(JOURNAL, padded_journal(case, JOURNAL_BOUND))


def realistic_inventory(count):
    return {
        f"tasks-{number:06d}.json": [16777231, 1000000 + number, 33188, 2048 + number, 1700000000123456789 + number]
        for number in range(count)
    }


def nested_inventory(depth):
    inventory = {}
    for level in reversed(range(depth)):
        inventory = {f"dir-{level}": [16777231, 5000 + level, 16877, 96, 1700000000123456789, inventory]}
    return inventory


def committed_by_marker(case, text):
    """A qualified journal whose operation the marker records: the journal is accepted."""
    case.write(MARKER, python_marker(recovery_operation_id=OPERATION))
    case.write(".harflow-store.lock", b"")
    case.write(JOURNAL, text)


def committed_large_journal(case):
    committed_by_marker(case, journal(case.state, "completed", inventory=realistic_inventory(20000)))


def committed_nested_journal(case):
    committed_by_marker(case, journal(case.state, "completed", inventory=nested_inventory(70)))


def committed_journal_at_the_bound(case):
    committed_by_marker(case, padded_journal(case, JOURNAL_BOUND))


def committed_journal_over_the_bound(case):
    committed_by_marker(case, padded_journal(case, JOURNAL_BOUND + 1))


def journal_is_unreadable(case):
    skip_unless_permissions_apply(case)
    case.write(JOURNAL, journal(case.state, "completed"))
    os.chmod(os.path.join(case.state, JOURNAL), 0)


def journal_is_a_fifo(case):
    if not hasattr(os, "mkfifo"):
        case.skipTest("FIFOs exist on Unix only")
    os.makedirs(case.state, exist_ok=True)
    os.mkfifo(os.path.join(case.state, JOURNAL))


def large_completed_journal(case):
    case.write(JOURNAL, journal(case.state, "completed", inventory=realistic_inventory(20000)))


def deeply_nested_completed_journal(case):
    case.write(JOURNAL, journal(case.state, "completed", inventory=nested_inventory(70)))


def interrupted_directory(case):
    make_dirs(case, JOURNAL)


def interrupted_pending_corrupt(case):
    case.write(JOURNAL, journal(case.state, "completed"))
    case.write(JOURNAL + ".pending", b"{")


def interrupted_on_rust_root(case):
    rust_root(case.state)
    case.write(JOURNAL, journal(case.state, "prepared"))


def completed_journal(case):
    case.write(JOURNAL, journal(case.state, "completed"))


def committed_journal_with_matching_marker(case):
    journal_with_marker(case, "committed", OPERATION)


def completed_journal_on_rust_root(case):
    rust_root(case.state)
    case.write(JOURNAL, journal(case.state, "completed"))


def root_is_symlink(case):
    target = os.path.join(case.base, "real")
    os.mkdir(target)
    case.symlink(target, case.state)


def root_is_dangling_symlink(case):
    case.symlink(os.path.join(case.base, "nowhere"), case.state)


def root_under_symlinked_ancestor(case):
    target = os.path.join(case.base, "real")
    os.makedirs(os.path.join(target, "state"))
    link = os.path.join(case.base, "link")
    case.symlink(target, link)
    return os.path.join(link, "state")


def root_is_a_file(case):
    case.write("state", b"x", root=case.base)


def root_is_home(case):
    return case.home


def root_is_filesystem_root(case):
    if os.name == "nt":
        case.skipTest("no single filesystem root on Windows")
    return os.sep


def symlinked_leaf(case, name, content):
    outside = os.path.join(case.base, "outside-" + name)
    with open(outside, "wb") as handle:
        handle.write(content)
    os.makedirs(case.state, exist_ok=True)
    case.symlink(outside, os.path.join(case.state, name))


def identity_is_a_symlink(case):
    rust_root(case.state)
    os.remove(os.path.join(case.state, "store.identity"))
    symlinked_leaf(case, "store.identity", identity_text(storage_constants()).encode())


def database_is_a_symlink(case):
    rust_root(case.state)
    path = os.path.join(case.state, "store.sqlite3")
    with open(path, "rb") as handle:
        content = handle.read()
    os.remove(path)
    symlinked_leaf(case, "store.sqlite3", content)


def journal_is_a_symlink(case):
    symlinked_leaf(case, JOURNAL, journal(case.state, "completed").encode())


def marker_is_a_symlink(case):
    symlinked_leaf(case, MARKER, python_marker().encode())


def root_without_search_permission(case):
    skip_unless_permissions_apply(case)
    make_dirs(case)
    os.chmod(case.state, 0o444)


def root_without_search_permission_and_content(case):
    skip_unless_permissions_apply(case)
    rust_root(case.state)
    case.write(JOURNAL, journal(case.state, "completed"))
    os.chmod(case.state, 0o444)


def skip_unless_permissions_apply(case):
    if os.name == "nt" or (hasattr(os, "geteuid") and os.geteuid() == 0):
        case.skipTest("permission bits do not deny the owner here")


def root_unreadable(case):
    skip_unless_permissions_apply(case)
    make_dirs(case)
    os.chmod(case.state, 0)
