#!/usr/bin/python3
"""Check exact-OID profile, branch identity, and Git ancestry with local fixtures."""

import contextlib
import hashlib
import io
import json
import os
import pathlib
import shutil
import stat
import sys
import tempfile
import types

TASK = "/root/.velnor-generator-preview-20261005"
FETCH = TASK + "/jackin-source-fetch-exact-v1.py"
FETCH_SHA256 = "7900007696749f67f4db0730238b3675c28eec39a9e8fb81c2a0b6aff02fb37b"
PROFILE = TASK + "/jackin-source-profile-4043d4e42d07.json"
PROFILE_SHA256 = "8bcd097c40ff484d234477e01d78f8f446718a9958aed0818396ba0ebe6e9e11"
TOKEN = "4043d4e42d07"


def fail(message):
    raise SystemExit("Jackin exact-source fixture: " + message)


def read_pinned(path, digest):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or st.st_uid != 0 or st.st_gid != 0:
            fail("pinned input type or owner changed")
        if st.st_mode & 0o022:
            fail("pinned input is writable by group or others")
        data = bytearray()
        while len(data) <= 65536:
            block = os.read(fd, min(4096, 65537 - len(data)))
            if not block:
                break
            data.extend(block)
        if len(data) > 65536 or hashlib.sha256(data).hexdigest() != digest:
            fail("pinned input digest or size changed")
        return bytes(data)
    finally:
        os.close(fd)


def load_fetch_module(data):
    module = types.ModuleType("jackin_exact_source_fixture_module")
    module.__file__ = FETCH
    exec(compile(data, FETCH, "exec"), module.__dict__)
    return module


def rejected(function, *args):
    try:
        function(*args)
    except SystemExit:
        return True
    return False


def main():
    if not sys.flags.isolated or not sys.flags.no_site:
        fail("run under Python -I -S")
    fetch = read_pinned(FETCH, FETCH_SHA256)
    profile_raw = read_pinned(PROFILE, PROFILE_SHA256)
    module = load_fetch_module(fetch)
    task_fd = module.task_fd()
    try:
        profile = module.read_profile(task_fd, TOKEN)
    finally:
        os.close(task_fd)
    if (profile["commit"], profile["tree"], profile["parent"]) != (
        "4043d4e42d07bd782677061f9f2a3f0872ba3219",
        "f7e9742a175720cf59a25107101702b99d6a6db8",
        "fe92af75e6b0da7ee90436df0b0f9af95e1af9e3",
    ):
        fail("immutable profile identities differ")
    if module.PROFILE_SHA256 != PROFILE_SHA256:
        fail("profile digest was not retained")

    good_ref = ("a" * 40 + "\t" + module.REMOTE_REF + "\n").encode()
    if module.parse_ls_remote(good_ref) != "a" * 40:
        fail("valid advertised ref was rejected")
    malformed_refs = [
        b"not-a-sha\t" + module.REMOTE_REF.encode() + b"\n",
        ("a" * 40 + "\trefs/heads/main\n").encode(),
        ("a" * 40 + "\t" + module.REMOTE_REF + "\n" + "b" * 40 + "\t" + module.REMOTE_REF + "\n").encode(),
        b"\xff\t" + module.REMOTE_REF.encode() + b"\n",
    ]
    if not all(rejected(module.parse_ls_remote, value) for value in malformed_refs):
        fail("malformed or ambiguous advertised refs were accepted")

    oid = "c" * 40
    if module.validate_stable_branch(oid, oid, oid, oid, oid) is not True:
        fail("stable exact-OID branch identity was rejected")
    moving_cases = [
        (oid, "d" * 40, oid, oid, oid),
        (oid, oid, "d" * 40, oid, oid),
        (oid, oid, oid, "d" * 40, oid),
        (oid, oid, oid, oid, "d" * 40),
    ]
    if not all(rejected(module.validate_stable_branch, *case) for case in moving_cases):
        fail("moving ref or mismatched ancestry was accepted")
    if not rejected(module.validate_stable_branch, "bad", oid, oid, oid, oid):
        fail("invalid requested object id was accepted")

    scratch = tempfile.mkdtemp(prefix="jackin-exact-source-fixture-v1-", dir=TASK)
    os.chmod(scratch, 0o700)
    try:
        module.TASK = scratch
        module.pinned_executable(module.GIT, module.GIT, module.GIT_SHA256, module.GIT_SIZE)
        repo = os.path.join(scratch, "repo")
        template = os.path.join(scratch, "template")
        os.mkdir(template, 0o700)
        with contextlib.redirect_stdout(io.StringIO()):
            module.run_git(["init", "--template=" + template, "--initial-branch=fixture", repo])
            module.run_git(["-C", repo, "config", "--local", "user.name", "Fixture"])
            module.run_git(["-C", repo, "config", "--local", "user.email", "fixture@example.invalid"])
            file_path = os.path.join(repo, "source.txt")
            fd = os.open(file_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
            try:
                os.write(fd, b"fixture source\n")
            finally:
                os.close(fd)
            module.run_git(["-C", repo, "add", "--", "source.txt"])
            module.run_git(["-C", repo, "commit", "--no-gpg-sign", "-m", "fixture source"])
            parent = module.run_git(["-C", repo, "rev-parse", "HEAD"]).decode().strip()
            module.run_git(["-C", repo, "update-ref", "refs/remotes/origin/fixture", parent])
            common = module.run_git(["-C", repo, "merge-base", parent, "refs/remotes/origin/fixture"]).decode().strip()
            module.validate_stable_branch(parent, parent, parent, parent, common)
            with open(file_path, "ab") as stream:
                stream.write(b"descendant\n")
            module.run_git(["-C", repo, "add", "--", "source.txt"])
            module.run_git(["-C", repo, "commit", "--no-gpg-sign", "-m", "fixture descendant"])
            tip = module.run_git(["-C", repo, "rev-parse", "HEAD"]).decode().strip()
            module.run_git(["-C", repo, "update-ref", "refs/remotes/origin/fixture", tip])
            common_ancestor = module.run_git(["-C", repo, "merge-base", parent, "refs/remotes/origin/fixture"]).decode().strip()
        if common_ancestor != parent:
            fail("Git did not identify requested commit as the advertised tip's ancestor")
        if not rejected(module.validate_stable_branch, parent, parent, tip, tip, common_ancestor):
            fail("stale requested commit was accepted as latest branch tip")
    finally:
        st = os.lstat(scratch)
        if not stat.S_ISDIR(st.st_mode) or stat.S_ISLNK(st.st_mode) or (st.st_uid, st.st_gid, stat.S_IMODE(st.st_mode)) != (0, 0, 0o700):
            fail("fixture scratch identity changed before cleanup")
        shutil.rmtree(scratch)

    print(json.dumps({
        "profile_sha256": PROFILE_SHA256,
        "fetch_source_sha256": FETCH_SHA256,
        "cases": {
            "exact_profile_binding": "PASS",
            "strict_advertised_ref_parse": "PASS",
            "branch_movement_and_ancestry_rejection": "PASS",
            "local_git_exact_head_and_ancestor": "PASS",
        },
        "network": "none",
        "source_execution": "only reviewed fetch helper functions and local system Git over a synthetic repo",
        "status": "PASS",
    }, sort_keys=True))


if __name__ == "__main__":
    main()
