import errno
import os
import selectors
import subprocess
import sys
import time

PYTHON = "/Library/Developer/CommandLineTools/usr/bin/python3"


def main():
    operation, target = sys.argv[1:3]
    if operation == "sandbox-parent":
        profile, cwd, descriptor = sys.argv[2:5]
        os.execv("/usr/bin/sandbox-exec", [
            "sandbox-exec", "-p", profile, "/usr/bin/env", "-C", cwd,
            PYTHON, "-I", "-S", __file__, "write-fd", descriptor,
        ])
    if operation in ("descendant", "shell", "alternate-exec", "nested-sandbox"):
        return launch_controlled_route(operation, target)
    if operation in ("hardlink-control", "hardlink-workspace"):
        return probe_hardlink(operation, target)
    if operation == "fork":
        return probe_fork()
    if operation == "posix-spawn":
        return probe_posix_spawn()
    report = sys.argv[3] if len(sys.argv) > 3 else None
    try:
        perform(operation, target)
    except OSError as error:
        result = permission_result(error)
    else:
        result = "RESULT allowed"
    if report:
        write_report(report, result)
        return 0
    return emit(result)


def perform(operation, target):
    if operation == "write":
        write_file(target)
    elif operation == "write-fd":
        fd = int(target)
        os.lseek(fd, 0, os.SEEK_SET)
        os.write(fd, b"changed")
    elif operation == "append":
        fd = os.open(target, os.O_WRONLY | os.O_APPEND)
        os.write(fd, b"appended")
        os.close(fd)
    elif operation == "unlink":
        os.unlink(target)
    elif operation == "rename":
        os.rename(target, target + ".moved")
    elif operation == "ancestor-escape":
        moved = target + ".moved"
        os.rename(target, moved)
        write_file(os.path.join(moved, "control", "bytes"))
    elif operation == "chmod":
        os.chmod(target, 0o777)
    elif operation == "symlink":
        os.symlink(target + ".target", target)
    else:
        raise ValueError("unknown fixed operation")


def probe_hardlink(operation, source):
    root = os.path.dirname(os.path.dirname(source))
    if operation == "hardlink-control":
        destination = os.path.join(root, "workspace", "nested", "control", "new-link")
    else:
        destination = os.path.join(root, "workspace", "new-link")
    try:
        os.link(source, destination)
    except OSError as error:
        result = permission_result(error)
    else:
        result = "RESULT allowed"
    return emit(result)


def launch_controlled_route(operation, target):
    root = os.path.dirname(os.path.dirname(target))
    report = os.path.join(root, "runtime", operation + ".result")
    if operation == "descendant":
        child = os.fork()
        if child == 0:
            try:
                perform("write", target)
                result = "RESULT allowed"
            except OSError as error:
                result = permission_result(error)
            write_report(report, result)
            os._exit(0)
    elif operation == "shell":
        child = os.posix_spawn("/bin/sh", [
            "sh", "-c", 'exec "$@"', "harflow-shell", PYTHON,
            "-I", "-S", __file__, "write", target, report,
        ], os.environ)
    elif operation == "alternate-exec":
        child = os.posix_spawn(PYTHON, [
            PYTHON, "-I", "-S", __file__, "write", target, report,
        ], os.environ)
    else:
        return nested_sandbox_route(target, report)
    status = wait_child(child)
    if status is None:
        return emit("RESULT unknown reason=child-wait-timeout")
    if os.waitstatus_to_exitcode(status) != 0:
        return emit("RESULT unsupported route=" + operation)
    try:
        with open(report, "rb") as stream:
            result = stream.read(256).decode("ascii")
    except OSError as error:
        return emit("RESULT error route=" + operation + " errno=" + str(error.errno))
    return emit(result)


def nested_sandbox_route(target, report):
    nested = '(version 1) (allow default)'
    child = subprocess.Popen(
        [
            "/usr/bin/sandbox-exec", "-p", nested, PYTHON, "-I", "-S",
            __file__, "write", target, report,
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    selector = selectors.DefaultSelector()
    stdout = bytearray()
    stderr = bytearray()
    for stream, buffer in ((child.stdout, stdout), (child.stderr, stderr)):
        os.set_blocking(stream.fileno(), False)
        selector.register(stream, selectors.EVENT_READ, buffer)
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline and (child.poll() is None or selector.get_map()):
        for key, _ in selector.select(0.05):
            chunk = os.read(key.fileobj.fileno(), 1024)
            if chunk:
                key.data.extend(chunk)
            else:
                selector.unregister(key.fileobj)
                key.fileobj.close()
    timed_out = child.poll() is None
    if timed_out:
        child.kill()
        child.wait(timeout=1)
    drain_deadline = time.monotonic() + 1
    while selector.get_map() and time.monotonic() < drain_deadline:
        for key, _ in selector.select(0.05):
            chunk = os.read(key.fileobj.fileno(), 1024)
            if chunk:
                key.data.extend(chunk)
            else:
                selector.unregister(key.fileobj)
                key.fileobj.close()
    if selector.get_map():
        return emit("RESULT unknown reason=nested-child-stream-timeout")
    if timed_out:
        return emit("RESULT unknown reason=nested-child-timeout")
    if child.returncode == 0:
        try:
            with open(report, "rb") as stream:
                return emit(stream.read(256).decode("ascii"))
        except OSError as error:
            return emit("RESULT error route=nested-sandbox errno=" + str(error.errno))
    if child.returncode == 71 and bytes(stderr).strip() == (
        b"sandbox-exec: sandbox_apply: Operation not permitted"
    ):
        return emit("RESULT unsupported nested-apply-refused exit=71")
    return emit("RESULT error nested-exit=" + str(child.returncode))


def write_report(report, result):
    fd = os.open(report, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    os.write(fd, result.encode("ascii"))
    os.close(fd)


def write_file(target):
    fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    os.write(fd, b"changed-by-alias")
    os.close(fd)


def permission_result(error):
    if error.errno == errno.EPERM:
        return "RESULT denied errno=" + str(error.errno)
    return "RESULT error errno=" + str(error.errno)


def wait_child(pid):
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        finished, status = os.waitpid(pid, os.WNOHANG)
        if finished == pid:
            return status
    return None


def probe_fork():
    try:
        child = os.fork()
    except OSError as error:
        return emit(permission_result(error))
    if child == 0:
        os._exit(0)
    status = wait_child(child)
    if status is None:
        return emit("RESULT unknown reason=child-wait-timeout")
    if os.waitstatus_to_exitcode(status) != 0:
        return emit("RESULT failure reason=child-exit")
    return emit("RESULT allowed")


def probe_posix_spawn():
    try:
        child = os.posix_spawn("/usr/bin/true", ["true"], os.environ)
    except OSError as error:
        return emit(permission_result(error))
    status = wait_child(child)
    if status is None:
        return emit("RESULT unknown reason=child-wait-timeout")
    if os.waitstatus_to_exitcode(status) != 0:
        return emit("RESULT failure reason=child-exit")
    return emit("RESULT allowed")


def emit(result):
    print(result, flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
