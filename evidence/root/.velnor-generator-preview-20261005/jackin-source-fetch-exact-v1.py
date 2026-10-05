#!/usr/bin/python3
"""Fetch one public Jackin ref into a private no-checkout Git repository."""

import ctypes
import hashlib
import json
import os
import re
import resource
import signal
import stat
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone

TASK = "/root/.velnor-generator-preview-20261005"
SOURCE_NAME = None
SOURCE = None
TEMPLATE_NAME = None
TEMPLATE = None
TRANSCRIPT = None
LOG_NAME = None
PROFILE_PATH = None
PROFILE_SHA256 = None
ORIGIN = "https://github.com/jackin-project/jackin.git"
BRANCH = "refactor/build-split-dco"
REMOTE_REF = "refs/heads/" + BRANCH
LOCAL_REF = "refs/remotes/origin/" + BRANCH
COMMIT = None
TREE = None
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


def profile_token_from_argv():
    if len(sys.argv) != 3 or sys.argv[1] != "--fetch":
        fail("usage: jackin-source-fetch-exact-v1.py --fetch PROFILE_TOKEN")
    token = sys.argv[2]
    if not re.fullmatch(r"[0-9a-f]{12}", token):
        fail("profile token must be the first twelve commit hex digits")
    return token


def early_boundary():
    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}:
        fail("entry requires the exact clean environment")
    if not sys.flags.isolated or not sys.flags.no_site or sys.version_info[:3] != (3, 13, 5):
        fail("entry requires pinned isolated Python 3.13.5 without site")
    if (os.geteuid(), os.getegid(), os.getgroups()) != (0, 0, [0]):
        fail("entry requires only the root identity")
    parent = task_fd()
    token = profile_token_from_argv()
    log_name = "jackin-source-" + token + "-fetch.log"
    log_fd = os.open(log_name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=parent)
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


def read_profile(parent_fd, token):
    global SOURCE_NAME, SOURCE, TEMPLATE_NAME, TEMPLATE, TRANSCRIPT
    global PROFILE_PATH, PROFILE_SHA256, COMMIT, TREE
    profile_name = "jackin-source-profile-" + token + ".json"
    fd = os.open(profile_name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent_fd)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0):
            fail("source profile is not a private regular task file")
        if stat.S_IMODE(st.st_mode) != 0o600 or st.st_size > 16384:
            fail("source profile mode or size differs")
        raw = bytearray()
        while len(raw) <= 16384:
            chunk = os.read(fd, min(4096, 16385 - len(raw)))
            if not chunk:
                break
            raw.extend(chunk)
        if len(raw) > 16384:
            fail("source profile exceeds the fixed limit")
    finally:
        os.close(fd)
    try:
        profile = json.loads(bytes(raw))
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("source profile is not valid UTF-8 JSON")
    required = {
        "schema", "profile_id", "repository_id", "repository", "origin", "branch",
        "remote_ref", "commit", "tree", "parent", "retrieved_at_utc", "signature_verified",
    }
    if not isinstance(profile, dict) or set(profile) != required:
        fail("source profile fields differ from the reviewed schema")
    if profile["schema"] != 1 or profile["profile_id"] != token:
        fail("source profile schema or identity differs")
    if (profile["repository_id"], profile["repository"], profile["origin"], profile["branch"], profile["remote_ref"]) != (
        1197700841, "jackin-project/jackin", ORIGIN, BRANCH, REMOTE_REF
    ):
        fail("source profile repository identity differs")
    for key in ("commit", "tree", "parent"):
        value = profile[key]
        if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value):
            fail("source profile has invalid " + key)
    if profile["profile_id"] != profile["commit"][:12]:
        fail("profile token does not match the requested commit")
    if not isinstance(profile["retrieved_at_utc"], str) or not profile["retrieved_at_utc"].endswith("Z"):
        fail("source profile retrieval time is invalid")
    if not isinstance(profile["signature_verified"], bool):
        fail("source profile signature status is invalid")
    COMMIT = profile["commit"]
    TREE = profile["tree"]
    SOURCE_NAME = "jackin-source-" + token + "-exact-v1"
    SOURCE = TASK + "/" + SOURCE_NAME
    TEMPLATE_NAME = "jackin-source-" + token + "-empty-template-v1"
    TEMPLATE = TASK + "/" + TEMPLATE_NAME
    TRANSCRIPT = TASK + "/jackin-source-" + token + "-exact-v1-fetch.json"
    PROFILE_PATH = TASK + "/" + profile_name
    PROFILE_SHA256 = hashlib.sha256(bytes(raw)).hexdigest()
    return profile


def parse_ls_remote(raw):
    try:
        lines = raw.decode("ascii").splitlines()
    except UnicodeDecodeError:
        fail("advertised ref response is not ASCII")
    if len(lines) != 1:
        fail("advertised branch ref did not return one exact record")
    fields = lines[0].split("\t")
    if len(fields) != 2 or fields[1] != REMOTE_REF or not re.fullmatch(r"[0-9a-f]{40}", fields[0]):
        fail("advertised branch ref record is malformed")
    return fields[0]


def validate_stable_branch(requested, before, fetched, after, common):
    if not all(isinstance(item, str) and re.fullmatch(r"[0-9a-f]{40}", item) for item in (requested, before, fetched, after, common)):
        fail("branch identity contains an invalid Git object id")
    if not (requested == before == fetched == after == common):
        fail("requested object is not the stable advertised branch tip and its ancestry")
    return True


def ls_remote_tip():
    raw = run_git(["ls-remote", "--exit-code", "--heads", ORIGIN, REMOTE_REF])
    return parse_ls_remote(raw)


def write_transcript(parent_fd, document):
    data = (json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode()
    name = os.path.basename(TRANSCRIPT)
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=parent_fd)
    try:
        view = memoryview(data)
        while view:
            count = os.write(fd, view)
            if count <= 0:
                fail("short private transcript write")
            view = view[count:]
        os.fsync(fd)
    finally:
        os.close(fd)
    return hashlib.sha256(data).hexdigest()


def main():
    token = profile_token_from_argv()
    early_boundary()
    pinned_executable("/usr/bin/python3", PYTHON_REALPATH, PYTHON_SHA256, PYTHON_SIZE)
    pinned_executable(GIT, GIT, GIT_SHA256, GIT_SIZE)
    pinned_executable("/usr/bin/env", ENV_REALPATH, ENV_SHA256, ENV_SIZE)
    if run_git(["--version"]) != GIT_VERSION:
        fail("host Git version differs from the reviewed version")
    parent_fd = task_fd()
    profile = read_profile(parent_fd, token)
    branch_tip_before = ls_remote_tip()
    if branch_tip_before != COMMIT:
        fail("advertised branch tip differs from the reviewed source profile")
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
            "-C", SOURCE, "fetch", "--no-tags", "--no-auto-gc", "--no-recurse-submodules",
            "--depth=1", "origin", COMMIT,
        ])
        if run_git(["-C", SOURCE, "cat-file", "-e", COMMIT + "^{commit}"]) != b"":
            fail("requested exact commit object check returned unexpected output")
        run_git([
            "-C", SOURCE, "fetch", "--no-tags", "--no-auto-gc", "--no-recurse-submodules",
            "--depth=1", "origin", "+" + REMOTE_REF + ":" + LOCAL_REF,
        ])
        run_git(["-C", SOURCE, "update-ref", "refs/heads/" + BRANCH, COMMIT])
        run_git(["-C", SOURCE, "symbolic-ref", "HEAD", "refs/heads/" + BRANCH])
        run_git(["-C", SOURCE, "config", "--local", "--replace-all", "branch." + BRANCH + ".remote", "origin"])
        run_git(["-C", SOURCE, "config", "--local", "--replace-all", "branch." + BRANCH + ".merge", REMOTE_REF])
        commit = run_git(["-C", SOURCE, "rev-parse", "HEAD"]).decode().strip()
        tree = run_git(["-C", SOURCE, "rev-parse", "HEAD^{tree}"]).decode().strip()
        parent = run_git(["-C", SOURCE, "rev-parse", "HEAD^"]).decode().strip()
        remote_commit = run_git(["-C", SOURCE, "rev-parse", LOCAL_REF]).decode().strip()
        common = run_git(["-C", SOURCE, "merge-base", COMMIT, LOCAL_REF]).decode().strip()
        branch_tip_after = ls_remote_tip()
        upstream = run_git(["-C", SOURCE, "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).decode().strip()
        if (commit, tree, parent) != (COMMIT, TREE, profile["parent"]):
            fail("requested Git commit, tree, or parent differs from the reviewed source profile")
        validate_stable_branch(COMMIT, branch_tip_before, remote_commit, branch_tip_after, common)
        if upstream != "origin/" + BRANCH:
            fail("canonical upstream branch metadata differs")
        run_git(["-C", SOURCE, "cat-file", "-e", COMMIT + "^{commit}"])
        run_git(["-C", SOURCE, "fsck", "--strict", "--full", "--no-reflogs", "--no-progress"])
        entries = sorted(os.listdir(SOURCE))
        if entries != [".git"]:
            fail("no-checkout clone contains an unexpected worktree path")
        document = {
            "schema": 2,
            "repository_id": profile["repository_id"],
            "origin": ORIGIN,
            "branch": BRANCH,
            "remote_ref": REMOTE_REF,
            "requested_commit": COMMIT,
            "commit": commit,
            "tree": tree,
            "parent": profile["parent"],
            "advertised_tip_before": branch_tip_before,
            "fetched_ref_tip": remote_commit,
            "advertised_tip_after": branch_tip_after,
            "ancestry_proof": "requested commit equals fetched ref tip, both equal advertised ref before and after; merge-base equals requested commit",
            "upstream": upstream,
            "profile_name": os.path.basename(PROFILE_PATH),
            "profile_sha256": PROFILE_SHA256,
            "retrieved_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            "git_version": GIT_VERSION.decode().strip(),
            "git_sha256": GIT_SHA256,
            "python_version": sys.version.split()[0],
            "python_sha256": PYTHON_SHA256,
            "credential_access": "none; public HTTPS fetch, env cleared, helper empty, terminal prompt disabled",
            "checkout_filters_or_hooks": "none; no checkout, template disabled, hooks path /dev/null",
        }
        transcript_sha = write_transcript(parent_fd, document)
        print("PASS: exact canonical Jackin commit=" + commit + " tree=" + tree + " transcript_sha256=" + transcript_sha)
    finally:
        os.close(parent_fd)


if __name__ == "__main__":
    main()
