#!/usr/bin/python3
"""Materialize one authenticated Git tree without checkout filters."""
import hashlib
import json
import os
import pathlib
import posixpath
import resource
import stat
import subprocess
import sys
import ctypes
import tempfile

TASK = "/root/.velnor-generator-preview-20261005"
SOURCE = TASK + "/jackin-source-b80f991d56c2-exact-v2"
MANIFEST = TASK + "/jackin-source-b80f991d56c2-tree-manifest.json"
AUDIT = TASK + "/jackin-source-git-config-audit-b80-v1.py"
AUDIT_SHA256 = "9d0b744ada440b8d4b6f0cccceaa98b2f3d1e3e6b7978e1c1c00190d467f5f42"
COMMIT = "b80f991d56c2c13f913bb444f9f05a36b16d37df"
TREE = "15a71f1adebd75dc217aa3a67fd7e0b78a38b2b6"
PARENT = "3bee4c7217837bc37b928840eb93e717cc2bbfbe"
BRANCH = "refactor/build-split-dco"
PROFILE_PATH = TASK + "/jackin-source-profile-b80f991d56c2.json"
PROFILE_SHA256 = "ad15804b9a0d3a8abd28ad445b22fcad9a3769f0c1417fbc381dc9ebceb893bc"
TRANSCRIPT_PATH = TASK + "/jackin-source-b80f991d56c2-exact-v2-fetch.json"
TRANSCRIPT_SHA256 = "1132d846aebe3111650c7da5f419eb98b0e6fd52dd4c35bc35bb2f4496bb442d"
ORIGIN = "https://github.com/jackin-project/jackin.git"
PYTHON_REALPATH = "/usr/bin/python3.13"
PYTHON_SHA256 = "889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9"
PYTHON_SIZE = 6832784
GIT_SHA256 = "356db14e102d68a1a37d8a1ac577dfd678d45d46e92f468bef8b7154e7bfdc60"
GIT_SIZE = 4082768
GIT_VERSION = b"git version 2.47.3\n"
ENV_SHA256 = "eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e"
ENV_SIZE = 56144
MAX_TOTAL_BLOBS = 256 << 20
MAX_BLOB = 64 << 20
GIT_ENV = {
    "HOME": "/nonexistent",
    "PATH": "/usr/bin:/bin",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_SYSTEM": "/dev/null",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_TERMINAL_PROMPT": "0",
    "GIT_OPTIONAL_LOCKS": "0",
    "GIT_NO_LAZY_FETCH": "1",
    "GIT_NO_REPLACE_OBJECTS": "1",
    "GIT_PROTOCOL_FROM_USER": "0",
    "LC_ALL": "C",
}


def fail(message):
    raise SystemExit("source materializer: " + message)


def secure_task_fd():
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        parts = TASK.split("/")[1:]
        for index, part in enumerate(parts):
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            st = os.fstat(child)
            final = index == len(parts) - 1
            if not stat.S_ISDIR(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0):
                os.close(child)
                fail("task path contains a non-root directory")
            if final:
                if stat.S_IMODE(st.st_mode) != 0o700:
                    os.close(child)
                    fail("task directory mode changed")
            elif st.st_mode & 0o022:
                os.close(child)
                fail("task ancestor is writable by group or others")
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def early_boundary():
    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}:
        fail("entry requires the exact clean environment")
    if not sys.flags.isolated or not sys.flags.no_site:
        fail("entry requires isolated no-site Python mode")
    if os.geteuid() != 0 or os.getegid() != 0 or any(group != 0 for group in os.getgroups()):
        fail("entry requires only the root identity")
    task_fd = secure_task_fd()
    if sys.argv[1:] == ["--materialize"]:
        log_name = "jackin-source-materialize-b80f991d56c2-attempt-1.log"
    elif sys.argv[1:] == ["--verify-partial"]:
        log_name = "jackin-source-partial-verify-b80f991d56c2.log"
    elif sys.argv[1:] == ["--finalize-partial"]:
        log_name = "jackin-source-partial-finalize-b80f991d56c2.log"
    else:
        log_name = "jackin-source-verify-b80f991d56c2.log"
    log_fd = os.open(
        log_name,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
        0o600,
        dir_fd=task_fd,
    )
    log_stat = os.fstat(log_fd)
    if not stat.S_ISREG(log_stat.st_mode) or log_stat.st_nlink != 1 or (log_stat.st_uid, log_stat.st_gid) != (0, 0) or stat.S_IMODE(log_stat.st_mode) != 0o600:
        os.close(log_fd)
        os.close(task_fd)
        fail("private source log path is unsafe")
    null_fd = os.open("/dev/null", os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    os.dup2(null_fd, 0)
    os.dup2(log_fd, 1)
    os.dup2(log_fd, 2)
    os.close(null_fd)
    os.close(log_fd)
    os.close(task_fd)
    close_range = getattr(ctypes.CDLL(None, use_errno=True), "close_range", None)
    if close_range is None:
        fail("host libc has no close_range")
    close_range.argtypes = [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
    close_range.restype = ctypes.c_int
    if close_range(3, 0xFFFFFFFF, 0) != 0:
        fail("could not close inherited file descriptors")
    resource.setrlimit(resource.RLIMIT_CPU, (300, 300))
    resource.setrlimit(resource.RLIMIT_FSIZE, (4 << 20, 4 << 20))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_NPROC, (64, 64))
    resource.setrlimit(resource.RLIMIT_AS, (2 << 30, 2 << 30))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)


def pinned_file(path, expected_sha, expected_size):
    real = os.path.realpath(path)
    expected_real = {
        "/usr/bin/python3": PYTHON_REALPATH,
        "/usr/bin/git": "/usr/bin/git",
        "/usr/bin/env": "/usr/bin/env",
    }.get(path)
    if expected_real is None or real != expected_real:
        fail("host executable resolution changed")
    if not os.path.isabs(path):
        fail("host executable path is not absolute")
    st = os.stat(real, follow_symlinks=False)
    if not stat.S_ISREG(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o755 or st.st_size != expected_size:
        fail("host executable type, owner, mode, or size changed")
    fd = os.open(real, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
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


def git(args, input_bytes=None, limit=MAX_TOTAL_BLOBS + (1 << 20)):
    def child_limits():
        resource.setrlimit(resource.RLIMIT_CPU, (60, 60))
        resource.setrlimit(resource.RLIMIT_FSIZE, (limit + 1, limit + 1))
        resource.setrlimit(resource.RLIMIT_NOFILE, (32, 32))
        resource.setrlimit(resource.RLIMIT_NPROC, (16, 16))
        resource.setrlimit(resource.RLIMIT_AS, (1 << 30, 1 << 30))
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))

    command = [
        "/usr/bin/git",
        "-c", "core.fsmonitor=false",
        "-c", "core.hooksPath=/dev/null",
        "-c", "diff.external=",
        "-C", SOURCE,
        *args,
    ]
    with tempfile.TemporaryFile(dir=TASK) as output:
        run_options = {
            "env": GIT_ENV,
            "stdout": output,
            "stderr": subprocess.DEVNULL,
            "timeout": 60,
            "check": False,
            "close_fds": True,
            "preexec_fn": child_limits,
        }
        if input_bytes is None:
            run_options["stdin"] = subprocess.DEVNULL
        else:
            run_options["input"] = input_bytes
        result = subprocess.run(command, **run_options)
        if result.returncode != 0:
            fail("Git command failed: " + " ".join(args[:3]))
        output.flush()
        size = os.fstat(output.fileno()).st_size
        if size > limit:
            fail("Git output exceeds the fixed size limit")
        output.seek(0)
        return output.read(limit + 1)


def safe_relative_path(raw):
    try:
        path = raw.decode("utf-8", "strict")
    except UnicodeDecodeError:
        fail("repository contains a non-UTF-8 tracked path")
    parts = path.split("/")
    if (
        not path
        or path.startswith("/")
        or "\\" in path
        or any(part in ("", ".", "..") for part in parts)
        or any(ord(ch) < 0x20 or ord(ch) == 0x7F for ch in path)
        or parts[0] == ".git"
    ):
        fail("repository contains an unsafe tracked path")
    return path, parts


def checked_source(require_empty_worktree):
    source = pathlib.Path(SOURCE)
    if not source.is_absolute() or source.is_symlink():
        fail("source path is not a direct absolute directory")
    st = source.lstat()
    if not stat.S_ISDIR(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o755:
        fail("source directory owner or mode changed")
    if require_empty_worktree and sorted(os.listdir(source)) != [".git"]:
        fail("fresh no-checkout clone contains unexpected worktree entries")
    gitdir = source / ".git"
    gst = gitdir.lstat()
    if not stat.S_ISDIR(gst.st_mode) or (gst.st_uid, gst.st_gid) != (0, 0) or gst.st_mode & 0o022:
        fail("direct Git metadata directory is unsafe")
    pinned_file("/usr/bin/python3", PYTHON_SHA256, PYTHON_SIZE)
    pinned_file("/usr/bin/git", GIT_SHA256, GIT_SIZE)
    pinned_file("/usr/bin/env", ENV_SHA256, ENV_SIZE)
    if sys.version_info[:3] != (3, 13, 5):
        fail("host Python version changed")
    if git(["--version"]) != GIT_VERSION:
        fail("host Git version changed")
    audit_fd = os.open(AUDIT, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    audit_stat = os.fstat(audit_fd)
    if not stat.S_ISREG(audit_stat.st_mode) or audit_stat.st_nlink != 1 or (audit_stat.st_uid, audit_stat.st_gid) != (0, 0) or stat.S_IMODE(audit_stat.st_mode) != 0o644:
        os.close(audit_fd)
        fail("Git config audit helper is not the reviewed immutable file")
    audit_digest = hashlib.sha256()
    while True:
        chunk = os.read(audit_fd, 1 << 20)
        if not chunk:
            break
        audit_digest.update(chunk)
    if audit_digest.hexdigest() != AUDIT_SHA256:
        os.close(audit_fd)
        fail("Git config audit helper digest changed")
    os.lseek(audit_fd, 0, os.SEEK_SET)
    audit = subprocess.run(
        ["/usr/bin/python3", "-I", "-S", "/proc/self/fd/" + str(audit_fd), SOURCE],
        env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        timeout=10,
        check=False,
        close_fds=True,
        pass_fds=(audit_fd,),
    )
    os.close(audit_fd)
    if audit.returncode != 0:
        fail("local Git configuration audit failed")
    if git(["rev-parse", "HEAD"]).decode().strip() != COMMIT:
        fail("source commit differs from the pinned task-branch selection")
    if git(["rev-parse", "HEAD^{tree}"]).decode().strip() != TREE:
        fail("source tree differs from the pinned task-branch selection")
    parse_raw_commit_identity(git(["cat-file", "commit", COMMIT], limit=1 << 20))
    verify_source_provenance()
    if git(["remote", "get-url", "origin"]).decode().strip() != ORIGIN:
        fail("source origin differs from the canonical repository")
    if git(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).decode().strip() != "origin/refactor/build-split-dco":
        fail("source upstream does not identify the selected canonical task branch")
    if require_empty_worktree:
        git(["config", "--local", "--replace-all", "core.hooksPath", "/dev/null"])
    if git(["config", "--local", "--get", "core.hooksPath"]).decode().strip() != "/dev/null":
        fail("local hook path is not disabled")
    info = gitdir / "info"
    if require_empty_worktree:
        gitdir_fd = os.open(gitdir, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            info_fd = open_or_create_directory(gitdir_fd, "info")
            os.close(info_fd)
        finally:
            os.close(gitdir_fd)
    ist = info.lstat()
    if not stat.S_ISDIR(ist.st_mode) or ist.st_uid != 0 or ist.st_gid != 0 or ist.st_mode & 0o022:
        fail("Git info directory is unsafe")
    attributes = info / "attributes"
    if attributes.exists() or attributes.is_symlink():
        fail("local Git attributes file is not permitted")
    exclude = info / "exclude"
    if exclude.exists() or exclude.is_symlink():
        est = exclude.lstat()
        if not stat.S_ISREG(est.st_mode) or (est.st_uid, est.st_gid) != (0, 0) or est.st_mode & 0o022:
            fail("local Git exclude file is unsafe")
        if require_empty_worktree:
            fd = os.open(exclude, os.O_WRONLY | os.O_TRUNC | os.O_NOFOLLOW | os.O_CLOEXEC)
            os.close(fd)
        elif est.st_size != 0:
            fail("local Git exclude file must remain empty")


def parse_tree(raw):
    entries = []
    seen = set()
    for record in raw.split(b"\0"):
        if not record:
            continue
        meta, separator, raw_path = record.partition(b"\t")
        fields = meta.split()
        if not separator or len(fields) != 3:
            fail("Git tree record is malformed")
        mode_raw, kind_raw, oid_raw = fields
        try:
            mode = mode_raw.decode("ascii")
            kind = kind_raw.decode("ascii")
            oid = oid_raw.decode("ascii")
        except UnicodeDecodeError:
            fail("Git tree record has invalid metadata")
        path, parts = safe_relative_path(raw_path)
        if path in seen:
            fail("Git tree contains a duplicate path")
        seen.add(path)
        if len(oid) != 40 or any(ch not in "0123456789abcdef" for ch in oid):
            fail("Git object identifier is malformed")
        if mode in ("100644", "100755", "120000") and kind == "blob":
            entries.append({"path": path, "parts": parts, "mode": mode, "type": "blob", "oid": oid})
        elif mode == "160000" and kind == "commit":
            entries.append({"path": path, "parts": parts, "mode": mode, "type": "gitlink", "oid": oid})
        else:
            fail("Git tree contains an unsupported entry type")
    if not entries:
        fail("Git tree is empty")
    file_paths = {entry["path"] for entry in entries}
    for entry in entries:
        parents = ["/".join(entry["parts"][:index]) for index in range(1, len(entry["parts"]))]
        if any(parent in file_paths for parent in parents):
            fail("Git tree has a file/directory path collision")
    return sorted(entries, key=lambda entry: entry["path"].encode("utf-8"))


def read_objects(entries):
    object_ids = sorted({entry["oid"] for entry in entries if entry["type"] == "blob"})
    payload = ("\n".join(object_ids) + "\n").encode()
    raw = git(["cat-file", "--batch"], payload, limit=MAX_TOTAL_BLOBS + 2 * len(object_ids) + (1 << 20))
    objects = {}
    cursor = 0
    total = 0
    for expected_oid in object_ids:
        newline = raw.find(b"\n", cursor)
        if newline < 0:
            fail("Git batch response header is truncated")
        fields = raw[cursor:newline].split()
        cursor = newline + 1
        if len(fields) != 3 or fields[0].decode() != expected_oid or fields[1] != b"blob":
            fail("Git batch response does not match a requested blob")
        try:
            size = int(fields[2])
        except ValueError:
            fail("Git blob size is invalid")
        if size < 0 or size > MAX_BLOB:
            fail("Git blob exceeds the per-object size limit")
        total += size
        if total > MAX_TOTAL_BLOBS or cursor + size >= len(raw) or raw[cursor + size] != 0x0A:
            fail("Git batch response is truncated or oversized")
        data = raw[cursor : cursor + size]
        cursor += size + 1
        object_hash = hashlib.sha1(b"blob " + str(size).encode() + b"\0" + data).hexdigest()
        if object_hash != expected_oid:
            fail("Git blob content does not match its object identifier")
        objects[expected_oid] = data
    if cursor != len(raw):
        fail("Git batch response has trailing data")
    return objects


def open_or_create_directory(parent_fd, name):
    created = False
    try:
        os.mkdir(name, 0o755, dir_fd=parent_fd)
        created = True
    except FileExistsError:
        pass
    fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent_fd)
    st = os.fstat(fd)
    if not stat.S_ISDIR(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0):
        os.close(fd)
        fail("worktree directory has an unsafe type or owner")
    if created and stat.S_IMODE(st.st_mode) != 0o755:
        os.fchmod(fd, 0o755)
        st = os.fstat(fd)
    if stat.S_IMODE(st.st_mode) != 0o755:
        os.close(fd)
        fail("worktree directory has an unsafe mode")
    return fd


def parent_fd(root_fd, parts):
    fd = os.dup(root_fd)
    for part in parts[:-1]:
        child = open_or_create_directory(fd, part)
        os.close(fd)
        fd = child
    return fd


def safe_symlink(path, target_bytes):
    try:
        target = target_bytes.decode("utf-8", "strict")
    except UnicodeDecodeError:
        fail("repository contains a non-UTF-8 symlink target")
    if not target or target.startswith("/") or "\\" in target or "\0" in target:
        fail("repository contains an unsafe symlink target")
    resolved = posixpath.normpath(posixpath.join(posixpath.dirname(path), target))
    if resolved in ("..", ".") or resolved.startswith("../") or resolved.startswith("/"):
        fail("repository symlink leaves the source tree")
    return target


def lfs_pointer_record(path, oid, data):
    prefix = b"version https://git-lfs.github.com/spec/v1\n"
    if not data.startswith(prefix):
        return None
    lines = data.decode("ascii", "strict").splitlines()
    if len(lines) != 3 or lines[0] != "version https://git-lfs.github.com/spec/v1":
        fail("Git LFS pointer has an unsupported format")
    if not lines[1].startswith("oid sha256:") or len(lines[1]) != len("oid sha256:") + 64:
        fail("Git LFS pointer has an invalid object identifier")
    lfs_oid = lines[1][len("oid sha256:"):]
    if any(char not in "0123456789abcdef" for char in lfs_oid):
        fail("Git LFS pointer has an invalid object identifier")
    if not lines[2].startswith("size ") or not lines[2][5:].isdigit():
        fail("Git LFS pointer has an invalid size")
    return {
        "path": path,
        "git_blob_oid": oid,
        "lfs_oid": lfs_oid,
        "lfs_size": int(lines[2][5:]),
        "pointer_kept": True,
    }


def materialize(root_fd, entries, objects):
    gitlinks = []
    lfs_pointers = []
    for entry in entries:
        path = entry["path"]
        parts = entry["parts"]
        if entry["type"] == "gitlink":
            fd = parent_fd(root_fd, parts)
            try:
                child = open_or_create_directory(fd, parts[-1])
                try:
                    if os.listdir(child):
                        fail("gitlink target directory is not a fresh empty directory")
                finally:
                    os.close(child)
            finally:
                os.close(fd)
            gitlinks.append({"path": path, "commit": entry["oid"], "content_materialized": False})
            continue

        data = objects[entry["oid"]]
        if entry["mode"] == "120000":
            target = safe_symlink(path, data)
            fd = parent_fd(root_fd, parts)
            try:
                os.symlink(target, parts[-1], dir_fd=fd)
            finally:
                os.close(fd)
            entry["symlink_target"] = target
        else:
            mode = 0o755 if entry["mode"] == "100755" else 0o644
            fd = parent_fd(root_fd, parts)
            try:
                file_fd = os.open(
                    parts[-1],
                    os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                    mode,
                    dir_fd=fd,
                )
                try:
                    view = memoryview(data)
                    while view:
                        written = os.write(file_fd, view)
                        if written <= 0:
                            fail("short write while materializing a tracked blob")
                        view = view[written:]
                    os.fchmod(file_fd, mode)
                    st = os.fstat(file_fd)
                    if (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != mode:
                        fail("materialized blob owner or mode changed")
                finally:
                    os.close(file_fd)
            finally:
                os.close(fd)
            lfs_record = lfs_pointer_record(path, entry["oid"], data)
            if lfs_record is not None:
                lfs_pointers.append(lfs_record)

    return gitlinks, lfs_pointers


def write_manifest(entries, gitlinks, lfs_pointers):
    config_sha = hash_regular(SOURCE + "/.git/config")
    index_sha = hash_regular(SOURCE + "/.git/index")
    document = {
        "schema": 1,
        "repository": ORIGIN,
        "commit": COMMIT,
        "tree": TREE,
        "branch": BRANCH,
        "parent": PARENT,
        "profile_sha256": PROFILE_SHA256,
        "fetch_transcript_sha256": TRANSCRIPT_SHA256,
        "source_git_config_sha256": config_sha,
        "source_git_index_sha256": index_sha,
        "host_tools": {
            "python": {"version": "3.13.5", "sha256": PYTHON_SHA256, "path": PYTHON_REALPATH},
            "git": {"version": "2.47.3", "sha256": GIT_SHA256, "path": "/usr/bin/git"},
            "env": {"sha256": ENV_SHA256, "path": "/usr/bin/env"},
        },
        "entries": [
            {key: value for key, value in item.items() if key not in ("parts",)}
            for item in entries
        ],
        "gitlinks": gitlinks,
        "lfs_pointers": lfs_pointers,
        "scope": "raw tracked Git blobs; no checkout filters or hooks executed",
    }
    data = (json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode()
    fd = os.open(MANIFEST, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        view = memoryview(data)
        while view:
            written = os.write(fd, view)
            if written <= 0:
                fail("short write for source manifest")
            view = view[written:]
    finally:
        os.close(fd)
    digest = hashlib.sha256(data).hexdigest()
    print(
        "PASS: raw Git tree materialized; commit=" + COMMIT
        + " tree=" + TREE
        + " entries=" + str(len(entries))
        + " gitlinks=" + str(len(gitlinks))
        + " lfs_pointers=" + str(len(lfs_pointers))
        + " manifest_sha256=" + digest
    )


def hash_regular(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or st.st_mode & 0o022:
            fail("Git metadata file has an unsafe type or owner")
        digest = hashlib.sha256()
        while True:
            chunk = os.read(fd, 1 << 20)
            if not chunk:
                break
            digest.update(chunk)
        return digest.hexdigest()
    finally:
        os.close(fd)


def read_manifest():
    fd = os.open(MANIFEST, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600 or st.st_size > 32 << 20:
            fail("source manifest has an unsafe type, owner, mode, or size")
        data = bytearray()
        while len(data) <= 32 << 20:
            chunk = os.read(fd, min(1 << 20, (32 << 20) + 1 - len(data)))
            if not chunk:
                break
            data.extend(chunk)
        if len(data) > 32 << 20:
            fail("source manifest exceeds the size limit")
        return json.loads(data)
    finally:
        os.close(fd)


def verify_source_provenance():
    expected = {
        PROFILE_PATH: PROFILE_SHA256,
        TRANSCRIPT_PATH: TRANSCRIPT_SHA256,
    }
    parsed = {}
    for path, digest in expected.items():
        if hash_regular(path) != digest:
            fail("source provenance file digest changed")
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            st = os.fstat(fd)
            if stat.S_IMODE(st.st_mode) != 0o600 or st.st_size > 16384:
                fail("source provenance file mode or size changed")
            data = bytearray()
            while len(data) <= 16384:
                chunk = os.read(fd, min(4096, 16385 - len(data)))
                if not chunk:
                    break
                data.extend(chunk)
        finally:
            os.close(fd)
        try:
            parsed[path] = json.loads(bytes(data))
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("source provenance file is not valid bounded JSON")
    profile = parsed[PROFILE_PATH]
    transcript = parsed[TRANSCRIPT_PATH]
    if (profile.get("repository_id"), profile.get("repository"), profile.get("origin"), profile.get("branch"), profile.get("commit"), profile.get("tree"), profile.get("parent")) != (
        1197700841, "jackin-project/jackin", ORIGIN, BRANCH, COMMIT, TREE, PARENT
    ):
        fail("source profile identity differs from the exact selected task commit")
    if profile.get("signature_verified") is not False:
        fail("source profile signature disposition changed")
    if (transcript.get("repository_id"), transcript.get("origin"), transcript.get("branch"), transcript.get("requested_commit"), transcript.get("commit"), transcript.get("tree"), transcript.get("parent"), transcript.get("profile_sha256")) != (
        1197700841, ORIGIN, BRANCH, COMMIT, COMMIT, TREE, PARENT, PROFILE_SHA256
    ):
        fail("source fetch transcript identity differs from the exact selected commit")
    if transcript.get("checkout_filters_or_hooks") != "none; no checkout, template disabled, hooks path /dev/null":
        fail("source fetch did not preserve the no-checkout policy")


def parse_raw_commit_identity(raw):
    if not isinstance(raw, bytes) or len(raw) > (1 << 20):
        fail("raw Git commit exceeds its fixed limit")
    header, separator, _message = raw.partition(b"\n\n")
    if not separator:
        fail("raw Git commit header is malformed")
    trees = [line[5:].decode("ascii", "strict") for line in header.splitlines() if line.startswith(b"tree ")]
    parents = [line[7:].decode("ascii", "strict") for line in header.splitlines() if line.startswith(b"parent ")]
    if len(trees) != 1 or len(parents) != 1 or (trees[0], parents[0]) != (TREE, PARENT):
        fail("raw Git commit tree or parent differs from the reviewed profile")

def verify_index(entries):
    raw = git(["ls-files", "--stage", "-z"], limit=32 << 20)
    actual = []
    for record in raw.split(b"\0"):
        if not record:
            continue
        meta, separator, raw_path = record.partition(b"\t")
        fields = meta.split()
        if not separator or len(fields) != 3 or fields[2] != b"0":
            fail("Git index has an invalid or conflicted entry")
        path, _ = safe_relative_path(raw_path)
        actual.append({"path": path, "mode": fields[0].decode(), "oid": fields[1].decode()})
    expected = [
        {"path": item["path"], "mode": item["mode"], "oid": item["oid"]}
        for item in entries
    ]
    if actual != expected:
        fail("Git index entries differ from the pinned tree")


def verify_worktree(root_fd, entries):
    expected_paths = {".git"}
    entry_by_path = {item["path"]: item for item in entries}
    for item in entries:
        expected_paths.add(item["path"])
        parts = item["parts"]
        for index in range(1, len(parts)):
            expected_paths.add("/".join(parts[:index]))

    found_paths = {".git"}
    lfs_pointers = []

    def walk(fd, prefix):
        with os.scandir(fd) as iterator:
            names = sorted((entry.name for entry in iterator), key=os.fsencode)
        for name in names:
            if not prefix and name == ".git":
                continue
            path = prefix + "/" + name if prefix else name
            if path not in expected_paths:
                fail("worktree contains an extra path outside the pinned Git tree")
            found_paths.add(path)
            st = os.stat(name, dir_fd=fd, follow_symlinks=False)
            if path in entry_by_path:
                item = entry_by_path[path]
                if item["type"] == "gitlink":
                    if not stat.S_ISDIR(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o755:
                        fail("gitlink directory changed")
                    child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                    try:
                        if os.listdir(child):
                            fail("gitlink directory must remain explicitly unmaterialized")
                    finally:
                        os.close(child)
                elif item["mode"] == "120000":
                    if not stat.S_ISLNK(st.st_mode) or os.readlink(name, dir_fd=fd) != item["symlink_target"]:
                        fail("worktree symlink differs from its raw Git blob")
                else:
                    mode = 0o755 if item["mode"] == "100755" else 0o644
                    if not stat.S_ISREG(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != mode or st.st_nlink != 1:
                        fail("worktree file type, owner, mode, or link count changed")
                    file_fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                    try:
                        size = 0
                        content_prefix = bytearray()
                        while True:
                            chunk = os.read(file_fd, 1 << 20)
                            if not chunk:
                                break
                            size += len(chunk)
                            if len(content_prefix) < 1024:
                                content_prefix.extend(chunk[: 1024 - len(content_prefix)])
                        os.lseek(file_fd, 0, os.SEEK_SET)
                        object_hash = hashlib.sha1(b"blob " + str(size).encode() + b"\0")
                        while True:
                            chunk = os.read(file_fd, 1 << 20)
                            if not chunk:
                                break
                            object_hash.update(chunk)
                        if object_hash.hexdigest() != item["oid"]:
                            fail("worktree file content differs from its Git blob")
                        pointer = lfs_pointer_record(path, item["oid"], bytes(content_prefix))
                        if pointer is not None:
                            lfs_pointers.append(pointer)
                    finally:
                        os.close(file_fd)
            elif stat.S_ISDIR(st.st_mode):
                if (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o755:
                    fail("implicit source directory changed owner or mode")
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                try:
                    walk(child, path)
                finally:
                    os.close(child)
            else:
                fail("worktree contains an unexpected leaf")

    walk(root_fd, "")
    if found_paths != expected_paths:
        fail("worktree does not contain every pinned tree path")
    return lfs_pointers


def verify_partial_tree():
    checked_source(require_empty_worktree=False)
    for path in (MANIFEST, SOURCE + "/.git/index"):
        try:
            os.lstat(path)
        except FileNotFoundError:
            continue
        fail("partial source state already contains a manifest or Git index")
    entries = parse_tree(git(["ls-tree", "-r", "-z", "--full-tree", "HEAD"], limit=32 << 20))
    objects = read_objects(entries)
    for item in entries:
        if item["mode"] == "120000":
            item["symlink_target"] = safe_symlink(item["path"], objects[item["oid"]])
    root_fd = os.open(SOURCE, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        lfs_pointers = verify_worktree(root_fd, entries)
    finally:
        os.close(root_fd)
    expected_lfs = []
    for item in entries:
        if item["type"] == "blob" and item["mode"] in ("100644", "100755"):
            record = lfs_pointer_record(item["path"], item["oid"], objects[item["oid"]])
            if record is not None:
                expected_lfs.append(record)
    if lfs_pointers != expected_lfs:
        fail("partial worktree LFS pointers differ from authenticated Git blobs")
    gitlinks = [
        {"path": item["path"], "commit": item["oid"], "content_materialized": False}
        for item in entries if item["type"] == "gitlink"
    ]
    print(
        "PASS: partial worktree matches authenticated raw Git tree; commit=" + COMMIT
        + " tree=" + TREE
        + " entries=" + str(len(entries))
        + " gitlinks=" + str(len(gitlinks))
        + " lfs_pointers=" + str(len(lfs_pointers))
    )
    return entries, gitlinks, lfs_pointers


def finalize_partial():
    entries, gitlinks, lfs_pointers = verify_partial_tree()
    git(["read-tree", "HEAD"])
    verify_index(entries)
    if git(["rev-parse", "HEAD"]).decode().strip() != COMMIT:
        fail("source commit changed after partial-tree verification")
    if git(["rev-parse", "HEAD^{tree}"]).decode().strip() != TREE:
        fail("source tree changed after partial-tree verification")
    write_manifest(entries, gitlinks, lfs_pointers)


def verify_materialized():
    checked_source(require_empty_worktree=False)
    manifest = read_manifest()
    if manifest.get("schema") != 1 or manifest.get("repository") != ORIGIN or manifest.get("commit") != COMMIT or manifest.get("tree") != TREE:
        fail("source manifest identity differs from the immutable Git selection")
    if manifest.get("scope") != "raw tracked Git blobs; no checkout filters or hooks executed":
        fail("source materialization scope differs")
    entries = parse_tree(git(["ls-tree", "-r", "-z", "--full-tree", "HEAD"], limit=32 << 20))
    objects = read_objects(entries)
    for item in entries:
        if item["mode"] == "120000":
            item["symlink_target"] = safe_symlink(item["path"], objects[item["oid"]])
    expected_gitlinks = [
        {"path": item["path"], "commit": item["oid"], "content_materialized": False}
        for item in entries if item["type"] == "gitlink"
    ]
    expected_entries = [
        {key: value for key, value in item.items() if key != "parts"}
        for item in entries
    ]
    if manifest.get("entries") != expected_entries:
        fail("manifest entries differ from the raw Git tree")
    if manifest.get("gitlinks") != expected_gitlinks:
        fail("manifest gitlink disclosure differs from the immutable tree")
    if manifest.get("source_git_config_sha256") != hash_regular(SOURCE + "/.git/config"):
        fail("local Git config differs from the source manifest")
    if manifest.get("source_git_index_sha256") != hash_regular(SOURCE + "/.git/index"):
        fail("local Git index differs from the source manifest")
    verify_index(entries)
    root_fd = os.open(SOURCE, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        lfs_pointers = verify_worktree(root_fd, entries)
    finally:
        os.close(root_fd)
    if manifest.get("lfs_pointers") != lfs_pointers:
        fail("manifest LFS pointer disclosure differs from tracked file content")
    print("PASS: source worktree, index, and Git object tree match the immutable commit")


def main():
    early_boundary()
    if sys.argv[1:] == ["--verify"]:
        if os.geteuid() != 0 or os.getegid() != 0:
            fail("verification requires the reviewed root boundary")
        verify_materialized()
        return
    if sys.argv[1:] == ["--verify-partial"]:
        if os.geteuid() != 0 or os.getegid() != 0:
            fail("partial verification requires the reviewed root boundary")
        verify_partial_tree()
        return
    if sys.argv[1:] == ["--finalize-partial"]:
        if os.geteuid() != 0 or os.getegid() != 0:
            fail("partial finalization requires the reviewed root boundary")
        finalize_partial()
        return
    if sys.argv[1:] != ["--materialize"]:
        fail("usage: source-materializer.py --materialize|--verify|--verify-partial|--finalize-partial")
    if os.geteuid() != 0 or os.getegid() != 0:
        fail("materialization requires the reviewed root staging boundary")
    checked_source(require_empty_worktree=True)
    git(["config", "--local", "--replace-all", "core.hooksPath", "/dev/null"])
    if git(["config", "--local", "--get", "core.hooksPath"]).decode().strip() != "/dev/null":
        fail("local hook path is not disabled")
    checked_source(require_empty_worktree=False)
    raw_tree = git(["ls-tree", "-r", "-z", "--full-tree", "HEAD"], limit=32 << 20)
    entries = parse_tree(raw_tree)
    objects = read_objects(entries)
    root_fd = os.open(SOURCE, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        gitlinks, lfs_pointers = materialize(root_fd, entries, objects)
        verify_lfs_pointers = verify_worktree(root_fd, entries)
    finally:
        os.close(root_fd)
    if verify_lfs_pointers != lfs_pointers:
        fail("staged LFS pointer inventory differs from the materialized tree")
    git(["read-tree", "HEAD"])
    verify_index(entries)
    if git(["rev-parse", "HEAD"]).decode().strip() != COMMIT:
        fail("source commit changed after materialization")
    if git(["rev-parse", "HEAD^{tree}"]).decode().strip() != TREE:
        fail("source tree changed after materialization")
    write_manifest(entries, gitlinks, lfs_pointers)


if __name__ == "__main__":
    main()
