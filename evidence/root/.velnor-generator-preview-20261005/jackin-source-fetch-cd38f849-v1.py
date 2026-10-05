#!/usr/bin/python3
"""Fetch one public Jackin ref into a private no-checkout Git repository."""

import ctypes
import hashlib
import json
import os
import resource
import signal
import stat
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone

TASK = "/root/.velnor-generator-preview-20261005"
SOURCE_NAME = "jackin-source-cd38f849-v1"
SOURCE = TASK + "/" + SOURCE_NAME
TEMPLATE_NAME = "jackin-source-cd38f849-empty-template-v1"
TEMPLATE = TASK + "/" + TEMPLATE_NAME
TRANSCRIPT = TASK + "/jackin-source-cd38f849-v1-fetch.json"
LOG_NAME = "jackin-source-cd38f849-v1-fetch.log"
ORIGIN = "https://github.com/jackin-project/jackin.git"
BRANCH = "refactor/build-split-dco"
REMOTE_REF = "refs/heads/" + BRANCH
LOCAL_REF = "refs/remotes/origin/" + BRANCH
COMMIT = "cd38f849396046dffe53f993f2b3f1e76d5d6906"
TREE = "95fa93fbb386106d4ba851e8fe62b84984f244b7"
GIT = "/usr/bin/git"
GIT_VERSION = b"git version 2.47.3\n"
GIT_SHA256 = "356db14e102d68a1a37d8a1ac577dfd678d45d46e92f468bef8b7154e7bfdc60"
GIT_SIZE = 4082768
PYTHON_REALPATH = "/usr/bin/python3.13"
PYTHON_SHA256 = "889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9"
PYTHON_SIZE = 6832784
ENV_REALPATH = "/usr/bin/env"
ENV_SHA256 = "eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e"
ENV_SIZE = 56144
GIT_ENV = {
    "HOME": "/nonexistent",
    "PATH": "/usr/bin:/bin",
    "LC_ALL": "C",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_SYSTEM": "/dev/null",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_TERMINAL_PROMPT": "0",
    "GIT_OPTIONAL_LOCKS": "0",
    "GIT_NO_LAZY_FETCH": "1",
    "GIT_NO_REPLACE_OBJECTS": "1",
    "GIT_PROTOCOL_FROM_USER": "0",
    "GIT_ALLOW_PROTOCOL": "https",
    "GIT_MAINTENANCE_AUTO": "0",
}


def fail(message):
    raise SystemExit("Jackin source fetch: " + message)


def pinned_executable(path, realpath, expected_sha, expected_size):
    if not os.path.isabs(path) or os.path.realpath(path) != realpath:
        fail("host executable path resolution changed")
    fd = os.open(realpath, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0):
            fail("host executable type or owner changed")
        if stat.S_IMODE(st.st_mode) != 0o755 or st.st_size != expected_size:
            fail("host executable mode or size changed")
        digest = hashlib.sha256()
        size = 0
        while True:
            chunk = os.read(fd, 1 << 20)
            if not chunk:
                break
            digest.update(chunk)
            size += len(chunk)
        if size != expected_size or digest.hexdigest() != expected_sha:
            fail("host executable digest changed")
    finally:
        os.close(fd)


def task_fd():
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        parts = TASK.split("/")[1:]
        for index, part in enumerate(parts):
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            st = os.fstat(child)
            if not stat.S_ISDIR(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0):
                os.close(child)
                fail("task path has an unsafe directory")
            if index == len(parts) - 1:
                if stat.S_IMODE(st.st_mode) != 0o700:
                    os.close(child)
                    fail("task directory mode changed")
            elif st.st_mode & 0o022:
                os.close(child)
                fail("task ancestor is group or world writable")
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def close_inherited_fds():
    close_range = getattr(ctypes.CDLL(None, use_errno=True), "close_range", None)
    if close_range is None:
        fail("host libc does not provide close_range")
    close_range.argtypes = [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
    close_range.restype = ctypes.c_int
    if close_range(3, 0xFFFFFFFF, 0) != 0:
        fail("could not close inherited descriptors")


def early_boundary():
    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}:
        fail("entry requires the exact clean environment")
    if not sys.flags.isolated or not sys.flags.no_site or sys.version_info[:3] != (3, 13, 5):
        fail("entry requires pinned isolated Python 3.13.5 without site")
    if (os.geteuid(), os.getegid(), os.getgroups()) != (0, 0, [0]):
        fail("entry requires only the root identity")
    parent = task_fd()
    log_fd = os.open(LOG_NAME, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=parent)
    log_st = os.fstat(log_fd)
    if not stat.S_ISREG(log_st.st_mode) or log_st.st_nlink != 1 or (log_st.st_uid, log_st.st_gid) != (0, 0) or stat.S_IMODE(log_st.st_mode) != 0o600:
        os.close(log_fd)
        os.close(parent)
        fail("private fetch log is unsafe")
    null_fd = os.open("/dev/null", os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    os.dup2(null_fd, 0)
    os.dup2(log_fd, 1)
    os.dup2(log_fd, 2)
    os.close(null_fd)
    os.close(log_fd)
    os.close(parent)
    close_inherited_fds()
    resource.setrlimit(resource.RLIMIT_CPU, (300, 300))
    resource.setrlimit(resource.RLIMIT_FSIZE, (512 << 20, 512 << 20))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_NPROC, (64, 64))
    resource.setrlimit(resource.RLIMIT_AS, (2 << 30, 2 << 30))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)


def run_git(args, timeout=180):
    command = [
        GIT,
        "-c", "core.fsmonitor=false",
        "-c", "core.hooksPath=/dev/null",
        "-c", "diff.external=",
        "-c", "credential.helper=",
        "-c", "http.sslVerify=true",
        "-c", "http.followRedirects=false",
        "-c", "protocol.version=2",
        "-c", "maintenance.auto=false",
        "-c", "gc.auto=0",
        "-c", "gc.autoDetach=false",
    ] + args
    with tempfile.TemporaryFile(dir=TASK) as output:
        process = None
        timed_out = False
        returncode = None

        def stop_process_group():
            if process is None:
                return
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            except OSError as exc:
                fail("could not terminate owned Git process group: " + str(exc.errno))
            time.sleep(0.05)
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            except OSError as exc:
                fail("could not kill owned Git process group: " + str(exc.errno))
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                fail("Git process did not exit after its process group was killed")

        try:
            process = subprocess.Popen(
                command,
                cwd=TASK,
                env=GIT_ENV,
                stdin=subprocess.DEVNULL,
                stdout=output,
                stderr=output,
                close_fds=True,
                start_new_session=True,
                preexec_fn=lambda: resource.setrlimit(resource.RLIMIT_CPU, (180, 180)),
            )
            try:
                returncode = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                stop_process_group()
                returncode = None
            else:
                stop_process_group()
            output.flush()
            st = os.fstat(output.fileno())
            if st.st_size > (8 << 20):
                fail("Git command output exceeds the fixed limit")
            output.seek(0)
            transcript = output.read((8 << 20) + 1)
        except BaseException:
            if process is not None:
                stop_process_group()
            raise
    print("argv=" + json.dumps(command, separators=(",", ":")))
    if transcript:
        print("output=" + transcript.decode("utf-8", "backslashreplace").rstrip())
    if timed_out:
        fail("Git command timed out and its process group was terminated")
    if returncode != 0:
        fail("Git command exited " + str(returncode))
    return transcript


def write_transcript(parent_fd, document):
    data = (json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode()
    fd = os.open("jackin-source-cd38f849-v1-fetch.json", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=parent_fd)
    try:
        view = memoryview(data)
        while view:
            count = os.write(fd, view)
            if count <= 0:
                fail("short private transcript write")
            view = view[count:]
    finally:
        os.close(fd)
    return hashlib.sha256(data).hexdigest()


def main():
    early_boundary()
    if sys.argv[1:] != ["--fetch"]:
        fail("usage: jackin-source-fetch-cd38f849-v1.py --fetch")
    pinned_executable("/usr/bin/python3", PYTHON_REALPATH, PYTHON_SHA256, PYTHON_SIZE)
    pinned_executable(GIT, GIT, GIT_SHA256, GIT_SIZE)
    pinned_executable("/usr/bin/env", ENV_REALPATH, ENV_SHA256, ENV_SIZE)
    if run_git(["--version"]) != GIT_VERSION:
        fail("host Git version differs from the reviewed version")
    parent_fd = task_fd()
    for name in (TEMPLATE_NAME, SOURCE_NAME):
        try:
            os.mkdir(name, 0o700 if name == TEMPLATE_NAME else 0o755, dir_fd=parent_fd)
        except FileExistsError:
            os.close(parent_fd)
            fail("task-owned source or empty-template destination already exists")
        fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent_fd)
        st = os.fstat(fd)
        expected_mode = 0o700 if name == TEMPLATE_NAME else 0o755
        if name == SOURCE_NAME and stat.S_IMODE(st.st_mode) != expected_mode:
            os.fchmod(fd, expected_mode)
            st = os.fstat(fd)
        if (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != expected_mode:
            os.close(fd)
            os.close(parent_fd)
            fail("fresh source or empty-template identity differs")
        os.close(fd)
    if os.listdir(TEMPLATE):
        os.close(parent_fd)
        fail("Git template directory is not empty")
    try:
        run_git(["init", "--template=" + TEMPLATE, "--initial-branch=" + BRANCH, SOURCE])
        run_git(["-C", SOURCE, "remote", "add", "origin", ORIGIN])
        run_git(["-C", SOURCE, "config", "--local", "--replace-all", "core.hooksPath", "/dev/null"])
        run_git(["-C", SOURCE, "config", "--local", "--replace-all", "credential.helper", ""])
        run_git(["-C", SOURCE, "config", "--local", "--replace-all", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"])
        run_git(["-C", SOURCE, "config", "--local", "--replace-all", "remote.origin.tagOpt", "--no-tags"])
        run_git([
            "-C", SOURCE, "fetch", "--no-tags", "--no-auto-gc", "--depth=1", "--no-recurse-submodules",
            "origin", "+" + REMOTE_REF + ":" + LOCAL_REF,
        ])
        run_git(["-C", SOURCE, "update-ref", "refs/heads/" + BRANCH, COMMIT])
        run_git(["-C", SOURCE, "symbolic-ref", "HEAD", "refs/heads/" + BRANCH])
        run_git(["-C", SOURCE, "config", "--local", "--replace-all", "branch." + BRANCH + ".remote", "origin"])
        run_git(["-C", SOURCE, "config", "--local", "--replace-all", "branch." + BRANCH + ".merge", REMOTE_REF])
        commit = run_git(["-C", SOURCE, "rev-parse", "HEAD"]).decode().strip()
        tree = run_git(["-C", SOURCE, "rev-parse", "HEAD^{tree}"]).decode().strip()
        remote_commit = run_git(["-C", SOURCE, "rev-parse", LOCAL_REF]).decode().strip()
        upstream = run_git(["-C", SOURCE, "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).decode().strip()
        if (commit, tree, remote_commit, upstream) != (COMMIT, TREE, COMMIT, "origin/" + BRANCH):
            fail("fetched Git identity differs from the immutable task input")
        run_git(["-C", SOURCE, "cat-file", "-e", COMMIT + "^{commit}"])
        run_git(["-C", SOURCE, "fsck", "--strict", "--full", "--no-reflogs", "--no-progress"])
        entries = sorted(os.listdir(SOURCE))
        if entries != [".git"]:
            fail("no-checkout clone contains an unexpected worktree path")
        document = {
            "schema": 1,
            "origin": ORIGIN,
            "branch": BRANCH,
            "remote_ref": REMOTE_REF,
            "commit": commit,
            "tree": tree,
            "remote_ref_commit": remote_commit,
            "upstream": upstream,
            "retrieved_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            "git_version": GIT_VERSION.decode().strip(),
            "git_sha256": GIT_SHA256,
            "python_version": sys.version.split()[0],
            "python_sha256": PYTHON_SHA256,
            "credential_access": "none; env cleared, helper empty, terminal prompt disabled",
            "checkout_filters_or_hooks": "none; no checkout, template disabled, hooks path /dev/null",
        }
        transcript_sha = write_transcript(parent_fd, document)
        print("PASS: fetched canonical Jackin commit=" + commit + " tree=" + tree + " transcript_sha256=" + transcript_sha)
    finally:
        os.close(parent_fd)


if __name__ == "__main__":
    main()
