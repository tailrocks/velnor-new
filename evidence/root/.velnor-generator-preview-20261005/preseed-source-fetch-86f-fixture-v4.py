#!/usr/bin/python3
"""Exercise the exact-source acquisition boundary without network access."""

import fcntl
import hashlib
import json
import os
import pathlib
import resource
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import types

TASK = "/root/.velnor-generator-preview-20261005"
SOURCE = TASK + "/preseed-source-fetch-86f-v2.py"
SOURCE_SHA256 = "593322f1fcdb3db045d9d10c1f0a2f3060238a8e366b2c129b5133be0dea49fb"
PYTHON = "/usr/bin/python3.13"
CLEAN_ENV = {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}


def fail(message):
    raise SystemExit("Velnor source-fetch fixture: " + message)


def load_source():
    fd = os.open(SOURCE, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        item = os.fstat(fd)
        if not stat.S_ISREG(item.st_mode) or (item.st_uid, item.st_gid) != (0, 0):
            fail("source type or owner changed")
        if stat.S_IMODE(item.st_mode) != 0o444 or item.st_nlink != 1:
            fail("source mode or link count changed")
        digest = hashlib.sha256()
        blocks = []
        while True:
            block = os.read(fd, 1 << 16)
            if not block:
                break
            digest.update(block)
            blocks.append(block)
        if digest.hexdigest() != SOURCE_SHA256:
            fail("source digest changed")
    finally:
        os.close(fd)
    module = types.ModuleType("velnor_fetch_fixture_module")
    module.__file__ = SOURCE
    exec(compile(b"".join(blocks), SOURCE, "exec"), module.__dict__)
    return module


def child_boundary(source, scratch, log_name, passed_fd, unsafe_mode):
    module = load_source()
    module.TASK = scratch
    module.LOG_NAME = log_name
    module.early_boundary()
    if passed_fd >= 0:
        try:
            fcntl.fcntl(passed_fd, fcntl.F_GETFD)
        except OSError as exc:
            if exc.errno != 9:
                fail("inherited descriptor returned an unexpected error")
        else:
            fail("inherited descriptor remained open")
    for descriptor in range(3, 64):
        try:
            fcntl.fcntl(descriptor, fcntl.F_GETFD)
        except OSError as exc:
            if exc.errno != 9:
                fail("descriptor probe returned an unexpected error")
        else:
            fail("unexpected descriptor remained open: " + str(descriptor))
    limits = {
        "cpu": resource.getrlimit(resource.RLIMIT_CPU),
        "fsize": resource.getrlimit(resource.RLIMIT_FSIZE),
        "nofile": resource.getrlimit(resource.RLIMIT_NOFILE),
        "nproc": resource.getrlimit(resource.RLIMIT_NPROC),
        "as": resource.getrlimit(resource.RLIMIT_AS),
        "core": resource.getrlimit(resource.RLIMIT_CORE),
    }
    old_umask = os.umask(0o777)
    os.umask(old_umask)
    expected = {
        "cpu": (300, 300),
        "fsize": (512 << 20, 512 << 20),
        "nofile": (64, 64),
        "nproc": (64, 64),
        "as": (2 << 30, 2 << 30),
        "core": (0, 0),
    }
    if limits != expected or old_umask != 0o077:
        fail("resource limits or umask differ")
    print(json.dumps({"fd_closed": True, "limits": limits, "umask": oct(old_umask), "status": "PASS"}, sort_keys=True))


def run_boundary_case(source_path, scratch, case, inherited_fd=False, extra_env=None, task_mode=0o700):
    case_dir = os.path.join(scratch, case)
    os.mkdir(case_dir, task_mode)
    os.chmod(case_dir, task_mode)
    command = [PYTHON, "-I", "-S", "-B", __file__, "--boundary-child", source_path, case_dir, case + ".log", "-1"]
    pass_fds = ()
    read_fd = write_fd = None
    if inherited_fd:
        read_fd, write_fd = os.pipe()
        command[-1] = str(write_fd)
        pass_fds = (write_fd,)
    environment = dict(CLEAN_ENV)
    if extra_env:
        environment.update(extra_env)
    try:
        result = subprocess.run(
            command,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            close_fds=True,
            pass_fds=pass_fds,
            timeout=15,
            check=False,
        )
    finally:
        if read_fd is not None:
            os.close(read_fd)
        if write_fd is not None:
            os.close(write_fd)
    if extra_env or task_mode != 0o700:
        if result.returncode == 0:
            fail("invalid environment or task mode unexpectedly passed")
        if extra_env and b"exact clean environment" not in result.stderr:
            fail("invalid environment failed for an unexpected reason")
        if task_mode != 0o700 and b"task directory mode changed" not in result.stderr:
            fail("invalid task mode failed for an unexpected reason")
        if os.path.lexists(os.path.join(case_dir, case + ".log")):
            fail("rejected boundary created a log")
        return {"case": case, "status": "PASS_REJECTED"}
    if result.returncode != 0:
        fail("valid boundary case failed: " + result.stderr.decode("utf-8", "replace"))
    log = os.path.join(case_dir, case + ".log")
    fd = os.open(log, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        item = os.fstat(fd)
        if (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink) != (0, 0, 0o600, 1):
            fail("boundary log identity differs")
        payload = os.read(fd, 4096)
    finally:
        os.close(fd)
    evidence = json.loads(payload)
    if evidence.get("status") != "PASS" or evidence.get("umask") != "0o77":
        fail("boundary child did not record the expected result")
    return {"case": case, "fd_case": inherited_fd, "status": "PASS"}


def directory_cases(module, scratch):
    old_umask = os.umask(0o077)
    parent = os.open(scratch, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        fd = module.create_task_directory(parent, "fresh-source", 0o755)
        if stat.S_IMODE(os.fstat(fd).st_mode) != 0o755:
            fail("new source directory was not normalized to 0755")
        os.close(fd)
        os.mkdir("existing-source", 0o700, dir_fd=parent)
        try:
            module.create_task_directory(parent, "existing-source", 0o755)
        except SystemExit:
            pass
        else:
            fail("pre-existing directory was accepted")
        current = os.stat("existing-source", dir_fd=parent, follow_symlinks=False)
        if stat.S_IMODE(current.st_mode) != 0o700:
            fail("pre-existing directory mode was changed")
        target = os.open("symlink-target", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent)
        os.close(target)
        os.symlink("symlink-target", "existing-link", dir_fd=parent)
        try:
            module.create_task_directory(parent, "existing-link", 0o755)
        except SystemExit:
            pass
        else:
            fail("pre-existing symlink was accepted")
        fifo = os.open("existing-fifo", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent)
        os.close(fifo)
        os.unlink("existing-fifo", dir_fd=parent)
        os.mkfifo("existing-fifo", 0o600, dir_fd=parent)
        try:
            module.create_task_directory(parent, "existing-fifo", 0o755)
        except SystemExit:
            pass
        else:
            fail("pre-existing FIFO was accepted")
    finally:
        os.close(parent)
        os.umask(old_umask)
    return {"fresh_mode": "0755", "preexisting_modes_preserved": True, "status": "PASS"}


def process_group_case(module, scratch):
    process_dir = os.path.join(scratch, "process-group")
    os.mkdir(process_dir, 0o700)
    module.TASK = process_dir
    fake = os.path.join(process_dir, "fake-git")
    with open(fake, "x", encoding="utf-8") as output:
        output.write(
            "#!/usr/bin/python3.13\n"
            "import json,os,signal,sys,time\n"
            "mode=sys.argv[-2]\n"
            "pid_path=sys.argv[-1]\n"
            "child=os.fork()\n"
            "if child == 0:\n"
            " signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
            " while True: time.sleep(1)\n"
            "fd=os.open(pid_path,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)\n"
            "os.write(fd,json.dumps({'leader':os.getpid(),'descendant':child}).encode())\n"
            "os.close(fd)\n"
            "if mode == 'exit': os._exit(17)\n"
            "while True: time.sleep(1)\n"
        )
    os.chmod(fake, 0o755)
    module.GIT = fake
    outcomes = {}
    for mode, timeout, expected in (("timeout", 1, "timed out"), ("exit", 5, "exited 17")):
        pid_path = os.path.join(process_dir, mode + ".pids.json")
        try:
            module.run_git(["--fixture", mode, pid_path], timeout=timeout)
        except SystemExit as exc:
            if expected not in str(exc):
                fail("unexpected process result for " + mode)
        else:
            fail("fake Git unexpectedly succeeded for " + mode)
        pids = json.loads(pathlib.Path(pid_path).read_text())
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            states = {}
            for key in ("leader", "descendant"):
                try:
                    raw = pathlib.Path("/proc", str(pids[key]), "stat").read_text()
                    states[key] = raw[raw.rfind(")") + 2 :].split()[0]
                except FileNotFoundError:
                    states[key] = "GONE"
            if all(value in ("GONE", "Z") for value in states.values()):
                break
            time.sleep(0.05)
        if not all(value in ("GONE", "Z") for value in states.values()):
            fail("fake Git process group survived " + mode)
        outcomes[mode] = {"states": states, "status": "PASS"}
    return outcomes


def main():
    if sys.argv[1:] and sys.argv[1] == "--boundary-child":
        if len(sys.argv) != 6:
            fail("boundary child arguments differ")
        child_boundary(sys.argv[2], sys.argv[3], sys.argv[4], int(sys.argv[5]), False)
        return
    if not sys.flags.isolated or not sys.flags.no_site or not sys.flags.dont_write_bytecode:
        fail("run fixture under Python -I -S -B")
    if (os.geteuid(), os.getegid(), os.getgroups()) != (0, 0, [0]):
        fail("fixture requires the root account")
    module = load_source()
    scratch = tempfile.mkdtemp(prefix="velnor-source-fetch-fixture-v3-", dir=TASK)
    os.chmod(scratch, 0o700)
    try:
        results = {
            "boundary_clean": run_boundary_case(SOURCE, scratch, "boundary-clean"),
            "boundary_inherited_fd": run_boundary_case(SOURCE, scratch, "boundary-inherited-fd", inherited_fd=True),
            "boundary_extra_env_rejected": run_boundary_case(SOURCE, scratch, "boundary-extra-env", extra_env={"UNSAFE_SENTINEL": "1"}),
            "boundary_unsafe_task_mode_rejected": run_boundary_case(SOURCE, scratch, "boundary-unsafe-mode", task_mode=0o755),
            "directories": directory_cases(module, scratch),
            "process_groups": process_group_case(module, scratch),
            "network": "none; only fake Git process groups ran",
        }
        print(json.dumps({"results": results, "status": "PASS"}, sort_keys=True))
    finally:
        shutil.rmtree(scratch)


if __name__ == "__main__":
    main()
