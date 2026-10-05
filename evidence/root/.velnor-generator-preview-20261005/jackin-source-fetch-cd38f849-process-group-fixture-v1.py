#!/usr/bin/python3
"""Exercise Git leader and descendant cleanup without network access."""

import hashlib
import json
import os
import pathlib
import signal
import stat
import sys
import tempfile
import time
import types

TASK = "/root/.velnor-generator-preview-20261005"
FETCH = TASK + "/jackin-source-fetch-cd38f849-v1.py"
FETCH_SHA256 = "2907bc1c84ccd36a05a652ea7003345de3b579b7bf35e6304806e80fb538da22"


def fail(message):
    raise SystemExit("Jackin fetch process-group fixture: " + message)


def read_source():
    fd = os.open(FETCH, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        item = os.fstat(fd)
        if not stat.S_ISREG(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink) != (0, 0, 0o644, 1):
            fail("fetch source metadata changed")
        chunks = []
        digest = hashlib.sha256()
        while True:
            block = os.read(fd, 1 << 16)
            if not block:
                break
            digest.update(block)
            chunks.append(block)
        if digest.hexdigest() != FETCH_SHA256:
            fail("fetch source digest changed")
        return b"".join(chunks)
    finally:
        os.close(fd)


def load_source(data):
    module = types.ModuleType("jackin_fetch_fixture_module")
    module.__file__ = FETCH
    exec(compile(data, FETCH, "exec"), module.__dict__)
    return module


def process_state(pid):
    try:
        raw = pathlib.Path(f"/proc/{pid}/stat").read_text()
    except FileNotFoundError:
        return "GONE"
    close = raw.rfind(")")
    fields = raw[close + 2:].split()
    return fields[0] if fields else "UNKNOWN"


def wait_terminated(pids):
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        states = {pid: process_state(pid) for pid in pids}
        if all(state in ("GONE", "Z") for state in states.values()):
            return states
        time.sleep(0.05)
    return {pid: process_state(pid) for pid in pids}


def main():
    if not sys.flags.isolated or not sys.flags.no_site:
        fail("run under Python -I -S")
    scratch = tempfile.mkdtemp(prefix="jackin-fetch-process-group-fixture-v1-", dir=TASK)
    os.chmod(scratch, 0o700)
    source = read_source()
    module = load_source(source)
    module.TASK = scratch
    fake_git = pathlib.Path(scratch, "fake-git")
    fake_git.write_text(
        "#!/usr/bin/python3\n"
        "import json,os,signal,sys,time\n"
        "mode=sys.argv[-2]\n"
        "pid_path=sys.argv[-1]\n"
        "child=os.fork()\n"
        "if child == 0:\n"
        " signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
        " while True: time.sleep(1)\n"
        "fd=os.open(pid_path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)\n"
        "os.write(fd,json.dumps({'leader':os.getpid(),'descendant':child}).encode())\n"
        "os.fsync(fd)\n"
        "os.close(fd)\n"
        "if mode == 'exit': os._exit(17)\n"
        "while True: time.sleep(1)\n"
    )
    os.chmod(fake_git, 0o755)
    module.GIT = str(fake_git)
    outcomes = {}
    for mode, timeout, expected in (("timeout", 1, "timed out"), ("exit", 5, "exited 17")):
        pid_path = os.path.join(scratch, mode + ".pids.json")
        try:
            module.run_git(["--fixture", mode, pid_path], timeout=timeout)
        except SystemExit as exc:
            if expected not in str(exc):
                fail("unexpected Git result for " + mode + ": " + str(exc))
        else:
            fail("fake Git unexpectedly succeeded for " + mode)
        pids = json.loads(pathlib.Path(pid_path).read_text())
        states = wait_terminated([pids["leader"], pids["descendant"]])
        if not all(state in ("GONE", "Z") for state in states.values()):
            fail("Git process group survived " + mode + ": " + repr(states))
        outcomes[mode] = {"exit_expected": expected, "process_states": states, "status": "PASS"}
    print(json.dumps({"cases": outcomes, "network": "none", "status": "PASS"}, sort_keys=True))


if __name__ == "__main__":
    main()
