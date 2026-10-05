#!/usr/bin/python3
"""Create a readable, sanitized Velnor 86f preview input from verified blobs."""

import hashlib
import json
import os
import pathlib
import posixpath
import resource
import stat
import sys

TASK = pathlib.Path("/root/.velnor-generator-preview-20261005")
PRIVATE = TASK / "generator-preview-86f"
SOURCE = TASK / "main-source-86f"
SOURCE_MANIFEST = TASK / "main-source-86f-manifest.json"
SOURCE_MANIFEST_SHA256 = "f12e0de7c5c22c5dc69e4f141d38d704a94f50494448dee3e896ea499a3e61b3"
SOURCE_MANIFEST_SIZE = 236097
COMMIT = "86f864aabc2192a9f8ccd3f01f2e25b7858c3f20"
TREE = "1e16a9270f3ea99b04e68521cfc876fa19db67ac"
REPOSITORY = "https://github.com/tailrocks/velnor-new.git"
INDEX_SHA256 = "b60394dee2640d075d013df7f5d3fbc47d6092aaa28f7e6a632e6f718672eb6a"
CONFIG_SHA256 = "49aa691b47210c0afb7c2a15642206965be75f7fc61580bf80b464ed22fc1873"
OUTPUT_NAME = "input-86f-v4"
RECORD_NAME = "input-86f-v4-record.json"
FETCH_JSON = "main-source-86f-fetch-v2.json"
FETCH_JSON_SHA256 = "aad27db0288e46f6a13ab6c4ec6fe86e88bb128e6da19b45620d7eab0af52b01"
FETCH_JSON_SIZE = 1303
FETCH_EVIDENCE = "main-source-86f-fetch-evidence-20261005T1721Z.json"
FETCH_EVIDENCE_SHA256 = "cae538dcc0a6e65be8209b115da176961eaa3b3344f1c6475dc1aabe7d05cb21"
FETCH_EVIDENCE_SIZE = 1877
FETCH_LOG = "main-source-86f-fetch-v2.log"
FETCH_LOG_SHA256 = "e64bc5b742a797b1e4e1f07658d1ea32aa8e469e4e4741a9fbbf2c64d9e82226"
FETCH_LOG_SIZE = 12243
MATERIALIZATION_EVIDENCE = "main-source-86f-materialize-evidence-20261005T1723Z.json"
MATERIALIZATION_EVIDENCE_SHA256 = "e1d0f8638b6beb15a004ebda64c26ec4924246ff4895431de3f59b03f90efe76"
MATERIALIZATION_EVIDENCE_SIZE = 2110
MATERIALIZATION_LOG = "source-materialize-86f.log"
MATERIALIZATION_LOG_SHA256 = "fb2095624d47c5c9092a8f471c9d4f9cfb529ec23a9d6031209cc2d9b7f3aa32"
MATERIALIZATION_LOG_SIZE = 247
SOURCE_VERIFY_LOG = "source-verify-86f.log"
SOURCE_VERIFY_LOG_SHA256 = "5f4741a32f7a940b3a4f46d9bb42ba21aa0cb54003aa10662bdb9a81988d2bcf"
SOURCE_VERIFY_LOG_SIZE = 77
OBSERVED_INDEX_SHA256 = "50ad81de6e038c3d5bf179442518709faf18f182cca35b4f8bd514e7f161e226"
PYTHON_RUNTIME_PATH = "/usr/bin/python3.13"
PYTHON_RUNTIME_REALPATH = "/usr/bin/python3.13"
PYTHON_RUNTIME_SHA256 = "889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9"
PYTHON_RUNTIME_SIZE = 6832784
FD_CLOSE_LAST = 0xFFFFFFFF
MAX_BLOB = 64 << 20
MAX_BLOBS_TOTAL = 256 << 20
MAX_OBJECTS = 128 << 20
MAX_TREE_ENTRIES = 20000
DIR_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
FILE_FLAGS = os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC
MINIMAL_GIT_CONFIG = (
    b"[core]\n"
    b"\trepositoryformatversion = 0\n"
    b"\tfilemode = true\n"
    b"\tbare = false\n"
    b"\thooksPath = /dev/null\n"
    b"[remote \"origin\"]\n"
    b"\turl = https://github.com/tailrocks/velnor-new.git\n"
    b"[branch \"main\"]\n"
    b"\tremote = origin\n"
    b"\tmerge = refs/heads/main\n"
)


class InputError(Exception):
    """A fail-closed source or destination validation error."""


def fail(message):
    raise InputError(message)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def git_blob_oid(data):
    header = b"blob " + str(len(data)).encode("ascii") + b"\0"
    return hashlib.sha1(header + data).hexdigest()


def valid_relpath(raw):
    if not isinstance(raw, str) or not raw or len(raw.encode("utf-8", "strict")) > 4096 or raw.startswith("/") or "\\" in raw or "\0" in raw:
        fail("invalid repository path")
    parts = raw.split("/")
    if any(part in ("", ".", "..", ".git") or len(part.encode("utf-8")) > 255 for part in parts):
        fail("unsafe repository path")
    if any(ord(ch) < 0x20 or ord(ch) == 0x7F for ch in raw):
        fail("control character in repository path")
    return parts


def valid_symlink(path, target):
    if not isinstance(target, str) or not target or target.startswith("/") or "\\" in target:
        fail("unsafe symlink target")
    resolved = posixpath.normpath(posixpath.join(posixpath.dirname(path), target))
    if resolved == ".." or resolved.startswith("../") or resolved.startswith("/"):
        fail("symlink escapes the source tree")
    if resolved == ".git" or resolved.startswith(".git/"):
        fail("source symlink targets Git metadata")


def canonical_tree(entries, gitlinks):
    if len(entries) + len(gitlinks) > MAX_TREE_ENTRIES:
        fail("source tree entry count exceeds its limit")
    root = {}
    seen = set()
    for original in entries:
        entry = dict(original)
        path = entry.get("path")
        parts = valid_relpath(path)
        if path in seen:
            fail("duplicate source path")
        seen.add(path)
        if entry.get("type") != "blob" or entry.get("mode") not in ("100644", "100755", "120000"):
            fail("unsupported Git blob entry")
        oid = entry.get("oid")
        if not isinstance(oid, str) or len(oid) != 40 or any(ch not in "0123456789abcdef" for ch in oid):
            fail("invalid blob object id")
        if entry["mode"] == "120000":
            valid_symlink(path, entry.get("symlink_target"))
        node = root
        for component in parts[:-1]:
            prior = node.get(component)
            if prior is not None and not isinstance(prior, dict):
                fail("file and directory path collision")
            node = node.setdefault(component, {})
        leaf = parts[-1]
        if leaf in node:
            fail("file and directory path collision")
        entry["path"] = path
        node[leaf] = ("entry", entry)
    for original in gitlinks:
        entry = dict(original)
        path = entry.get("path")
        parts = valid_relpath(path)
        if path in seen:
            fail("duplicate source path")
        seen.add(path)
        oid = entry.get("commit")
        if not isinstance(oid, str) or len(oid) != 40 or any(ch not in "0123456789abcdef" for ch in oid):
            fail("invalid gitlink commit")
        node = root
        for component in parts[:-1]:
            prior = node.get(component)
            if prior is not None and not isinstance(prior, dict):
                fail("gitlink path collision")
            node = node.setdefault(component, {})
        if parts[-1] in node:
            fail("gitlink path collision")
        node[parts[-1]] = ("entry", {"mode": "160000", "oid": oid, "path": path, "type": "gitlink"})

    def tree_oid(node):
        items = []
        for name, entry in node.items():
            if isinstance(entry, dict):
                mode, oid, order_name = "40000", tree_oid(entry), name.encode() + b"/"
            else:
                entry = entry[1]
                mode, oid, order_name = entry["mode"], entry["oid"], name.encode()
            items.append((order_name, mode, name, oid))
        body = b"".join(
            mode.encode("ascii") + b" " + name.encode("utf-8") + b"\0" + bytes.fromhex(oid)
            for _, mode, name, oid in sorted(items, key=lambda item: item[0])
        )
        return hashlib.sha1(b"tree " + str(len(body)).encode("ascii") + b"\0" + body).hexdigest()

    return root, seen, tree_oid(root)


def open_abs_dir(path):
    value = pathlib.PurePosixPath(path)
    if not value.is_absolute() or any(part in (".", "..") for part in value.parts[1:]):
        fail("invalid absolute path")
    fd = os.open("/", DIR_FLAGS)
    try:
        for part in value.parts[1:]:
            child = os.open(part, DIR_FLAGS, dir_fd=fd)
            st = os.fstat(child)
            if not stat.S_ISDIR(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or st.st_mode & 0o022:
                os.close(child)
                fail("unsafe absolute path component")
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def open_rel_parent(root_fd, path):
    parts = valid_relpath(path)
    fd = os.dup(root_fd)
    try:
        for part in parts[:-1]:
            child = os.open(part, DIR_FLAGS, dir_fd=fd)
            os.close(fd)
            fd = child
        return fd, parts[-1]
    except BaseException:
        os.close(fd)
        raise


def read_rel_regular(root_fd, path, limit):
    parent, leaf = open_rel_parent(root_fd, path)
    try:
        fd = os.open(leaf, FILE_FLAGS, dir_fd=parent)
    finally:
        os.close(parent)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or st.st_size > limit:
            fail("unsafe source file: " + path)
        chunks = []
        total = 0
        while True:
            block = os.read(fd, min(1 << 20, limit + 1 - total))
            if not block:
                break
            total += len(block)
            if total > limit:
                fail("source file exceeds limit: " + path)
            chunks.append(block)
        if total != st.st_size:
            fail("source file changed while reading: " + path)
        return b"".join(chunks), st
    finally:
        os.close(fd)


def read_rel_symlink(root_fd, path):
    parent, leaf = open_rel_parent(root_fd, path)
    try:
        st = os.stat(leaf, dir_fd=parent, follow_symlinks=False)
        if not stat.S_ISLNK(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0):
            fail("unsafe source symlink: " + path)
        return os.readlink(leaf, dir_fd=parent)
    finally:
        os.close(parent)


def mkdir_path(root_fd, path, mode=0o755):
    parts = valid_relpath(path)
    fd = os.dup(root_fd)
    try:
        for part in parts:
            created = False
            try:
                os.mkdir(part, mode, dir_fd=fd)
                created = True
            except FileExistsError:
                pass
            child = os.open(part, DIR_FLAGS, dir_fd=fd)
            st = os.fstat(child)
            if created and stat.S_ISDIR(st.st_mode):
                os.fchmod(child, mode)
                st = os.fstat(child)
            if not stat.S_ISDIR(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != mode:
                os.close(child)
                fail("unsafe destination directory")
            os.close(fd)
            fd = child
    finally:
        os.close(fd)


def write_file(root_fd, path, data, mode):
    parent, leaf = open_rel_parent(root_fd, path)
    try:
        fd = os.open(leaf, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, mode, dir_fd=parent)
        try:
            view = memoryview(data)
            while view:
                count = os.write(fd, view)
                if count <= 0:
                    fail("short destination write")
                view = view[count:]
            os.fchmod(fd, mode)
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        os.close(parent)


def write_symlink(root_fd, path, target):
    valid_relpath(path)
    valid_symlink(path, target)
    parent, leaf = open_rel_parent(root_fd, path)
    try:
        os.symlink(target, leaf, dir_fd=parent)
    finally:
        os.close(parent)


def source_manifest_bytes(task_fd):
    fd = os.open("main-source-86f-manifest.json", FILE_FLAGS, dir_fd=task_fd)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600 or st.st_nlink != 1 or st.st_size != SOURCE_MANIFEST_SIZE:
            fail("source manifest identity changed")
        data = read_fd(fd, st.st_size, SOURCE_MANIFEST_SIZE, "source manifest")
        if len(data) != SOURCE_MANIFEST_SIZE or sha256(data) != SOURCE_MANIFEST_SHA256:
            fail("source manifest digest changed")
        return data
    finally:
        os.close(fd)


def pinned_task_file(task_fd, name, expected_sha256, expected_size):
    fd = os.open(name, FILE_FLAGS, dir_fd=task_fd)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600 or st.st_nlink != 1 or st.st_size != expected_size:
            fail("pinned evidence file identity changed: " + name)
        data = read_fd(fd, st.st_size, expected_size, name)
        if sha256(data) != expected_sha256:
            fail("pinned evidence file digest changed: " + name)
        return data
    finally:
        os.close(fd)


def verify_source_evidence(task_fd):
    fetch_data = pinned_task_file(task_fd, FETCH_JSON, FETCH_JSON_SHA256, FETCH_JSON_SIZE)
    fetch = json.loads(fetch_data)
    if (
        fetch.get("schema") != 2
        or fetch.get("selected_commit") != COMMIT
        or fetch.get("tree") != TREE
        or fetch.get("origin") != REPOSITORY
        or fetch.get("selected_ref") != "refs/heads/main"
        or fetch.get("fetched_main_tip") != "540e12a4a225683e78378e63bfdbba2ddba29d86"
        or fetch.get("main_tip_stable_during_fetch") is not True
        or fetch.get("scope") != "historical exact-main preview source; current main-tip observations are recorded separately"
    ):
        fail("source fetch evidence does not bind the historical commit")
    fetch_evidence = json.loads(pinned_task_file(task_fd, FETCH_EVIDENCE, FETCH_EVIDENCE_SHA256, FETCH_EVIDENCE_SIZE))
    fetch_log = pinned_task_file(task_fd, FETCH_LOG, FETCH_LOG_SHA256, FETCH_LOG_SIZE)
    if (
        fetch_evidence.get("status") != "PASS"
        or fetch_evidence.get("exit_code") != 0
        or fetch_evidence.get("source", {}).get("commit") != COMMIT
        or fetch_evidence.get("source", {}).get("tree") != TREE
        or fetch_evidence.get("fetch_log", {}).get("sha256") != FETCH_LOG_SHA256
    ):
        fail("source fetch evidence record is inconsistent")
    required_transcript_fields = (
        b'"cat-file","commit","' + COMMIT.encode("ascii") + b'"',
        b'"fsck","--strict","--full","--no-reflogs","--no-progress"',
        b"PASS: exact Velnor commit=" + COMMIT.encode("ascii") + b" tree=" + TREE.encode("ascii"),
    )
    if any(field not in fetch_log for field in required_transcript_fields):
        fail("source fetch transcript lacks the exact commit/tree object validation")
    materialized = json.loads(pinned_task_file(task_fd, MATERIALIZATION_EVIDENCE, MATERIALIZATION_EVIDENCE_SHA256, MATERIALIZATION_EVIDENCE_SIZE))
    if (
        materialized.get("status") != "PASS"
        or materialized.get("source", {}).get("commit") != COMMIT
        or materialized.get("source", {}).get("tree") != TREE
        or materialized.get("manifest", {}).get("sha256") != SOURCE_MANIFEST_SHA256
    ):
        fail("source materialization evidence does not bind the frozen source")
    pinned_task_file(task_fd, MATERIALIZATION_LOG, MATERIALIZATION_LOG_SHA256, MATERIALIZATION_LOG_SIZE)
    pinned_task_file(task_fd, SOURCE_VERIFY_LOG, SOURCE_VERIFY_LOG_SHA256, SOURCE_VERIFY_LOG_SIZE)
    return {
        "fetch_record": {"path": FETCH_JSON, "sha256": FETCH_JSON_SHA256},
        "fetch_evidence": {"path": FETCH_EVIDENCE, "sha256": FETCH_EVIDENCE_SHA256},
        "fetch_log": {"path": FETCH_LOG, "sha256": FETCH_LOG_SHA256},
        "materialization_record": {"path": MATERIALIZATION_EVIDENCE, "sha256": MATERIALIZATION_EVIDENCE_SHA256},
        "materialization_log": {"path": MATERIALIZATION_LOG, "sha256": MATERIALIZATION_LOG_SHA256},
        "source_verify_log": {"path": SOURCE_VERIFY_LOG, "sha256": SOURCE_VERIFY_LOG_SHA256},
        "fetched_git_object_check": "exact commit/tree cat-file and git fsck --strict transcript fields verified by pinned log digest",
    }


def source_layout(root_fd, expected_paths):
    if len(expected_paths) > MAX_TREE_ENTRIES * 2:
        fail("source path inventory exceeds its limit")
    actual = set()

    def walk(fd, prefix):
        try:
            names = sorted(os.listdir(fd), key=os.fsencode)
        except UnicodeError:
            fail("source directory has a non-UTF-8 entry")
        for name in names:
            if name == ".git" and prefix == "":
                continue
            if name in (".", "..") or "/" in name:
                fail("invalid source directory entry")
            path = posixpath.join(prefix, name) if prefix else name
            try:
                path.encode("utf-8", "strict")
            except UnicodeError:
                fail("source path is not valid UTF-8")
            st = os.stat(name, dir_fd=fd, follow_symlinks=False)
            actual.add(path)
            if len(actual) > MAX_TREE_ENTRIES * 2:
                fail("source directory entry count exceeds its limit")
            if stat.S_ISDIR(st.st_mode):
                if (st.st_uid, st.st_gid, stat.S_IMODE(st.st_mode)) != (0, 0, 0o755):
                    fail("source directory identity changed")
                child = os.open(name, DIR_FLAGS, dir_fd=fd)
                try:
                    walk(child, path)
                finally:
                    os.close(child)
            elif stat.S_ISREG(st.st_mode) or stat.S_ISLNK(st.st_mode):
                if (st.st_uid, st.st_gid) != (0, 0) or st.st_nlink != 1:
                    fail("source file owner changed")
            else:
                fail("unsupported source filesystem entry")

    walk(root_fd, "")
    if actual != expected_paths:
        fail("materialized source path inventory differs from manifest")


def read_fd(fd, expected_size, limit, label):
    if expected_size < 0 or expected_size > limit:
        fail(label + " exceeds its size limit")
    chunks = []
    total = 0
    while total < expected_size:
        chunk = os.read(fd, min(1 << 20, expected_size - total))
        if not chunk:
            break
        chunks.append(chunk)
        total += len(chunk)
    if total != expected_size:
        fail(label + " changed while reading")
    if os.read(fd, 1):
        fail(label + " grew while reading")
    return b"".join(chunks)


def close_inherited_descriptors():
    import ctypes

    close_range = getattr(ctypes.CDLL(None, use_errno=True), "close_range", None)
    if close_range is None:
        fail("host libc does not provide close_range")
    close_range.argtypes = [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
    close_range.restype = ctypes.c_int
    if close_range(3, FD_CLOSE_LAST, 0) != 0:
        fail("could not close all inherited file descriptors")


def verify_python_runtime():
    if sys.executable != PYTHON_RUNTIME_PATH or os.path.realpath(sys.executable) != PYTHON_RUNTIME_REALPATH:
        fail("Python executable path differs from the pinned runtime")
    fd = os.open(PYTHON_RUNTIME_PATH, FILE_FLAGS)
    try:
        st = os.fstat(fd)
        if (
            not stat.S_ISREG(st.st_mode)
            or (st.st_uid, st.st_gid) != (0, 0)
            or stat.S_IMODE(st.st_mode) != 0o755
            or st.st_nlink != 1
            or st.st_size != PYTHON_RUNTIME_SIZE
        ):
            fail("pinned Python runtime file identity changed")
        data = read_fd(fd, st.st_size, 16 << 20, "pinned Python runtime")
        if sha256(data) != PYTHON_RUNTIME_SHA256:
            fail("pinned Python runtime digest changed")
    finally:
        os.close(fd)


def copy_objects(source_git_fd, dest_root_fd):
    source_objects = os.open("objects", DIR_FLAGS, dir_fd=source_git_fd)
    dest_git = os.open(".git", DIR_FLAGS, dir_fd=dest_root_fd)
    try:
        source_stat = os.fstat(source_objects)
        if (source_stat.st_uid, source_stat.st_gid, stat.S_IMODE(source_stat.st_mode)) != (0, 0, 0o700):
            fail("source Git object root identity changed")
        top = sorted(os.listdir(source_objects), key=os.fsencode)
        if top != ["info", "pack"]:
            fail("unexpected Git object root contents")
        info_fd = os.open("info", DIR_FLAGS, dir_fd=source_objects)
        try:
            info_stat = os.fstat(info_fd)
            if (info_stat.st_uid, info_stat.st_gid, stat.S_IMODE(info_stat.st_mode)) != (0, 0, 0o700) or os.listdir(info_fd):
                fail("Git object info data, including alternates, is not permitted")
        finally:
            os.close(info_fd)
        os.mkdir("objects", 0o755, dir_fd=dest_git)
        dest_objects = os.open("objects", DIR_FLAGS, dir_fd=dest_git)
        try:
            os.fchmod(dest_objects, 0o755)
            records = []
            total = 0

            def copy_pack_directory(src_fd, dst_fd):
                nonlocal total
                pack_stat = os.fstat(src_fd)
                if (pack_stat.st_uid, pack_stat.st_gid, stat.S_IMODE(pack_stat.st_mode)) != (0, 0, 0o700):
                    fail("source Git pack directory identity changed")
                source_names = sorted(os.listdir(src_fd), key=os.fsencode)
                if not source_names or any(not name.endswith((".pack", ".idx", ".rev")) for name in source_names):
                    fail("unexpected Git pack file name")
                pack_stems = {name.rsplit(".", 1)[0] for name in source_names}
                if any(
                    not stem.startswith("pack-")
                    or len(stem) != 45
                    or any(ch not in "0123456789abcdef" for ch in stem[5:])
                    for stem in pack_stems
                ):
                    fail("unexpected Git pack file prefix")
                names = [name for name in source_names if name.endswith((".pack", ".idx"))]
                for name in names:
                    st = os.stat(name, dir_fd=src_fd, follow_symlinks=False)
                    if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o400 or st.st_size > MAX_OBJECTS:
                        fail("unsafe Git pack file")
                    src_file = os.open(name, FILE_FLAGS, dir_fd=src_fd)
                    try:
                        data = read_fd(src_file, st.st_size, MAX_OBJECTS, "Git pack file")
                    finally:
                        os.close(src_file)
                    total += len(data)
                    if total > MAX_OBJECTS:
                        fail("Git object directory exceeds its size limit")
                    out = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o444, dir_fd=dst_fd)
                    try:
                        view = memoryview(data)
                        while view:
                            count = os.write(out, view)
                            if count <= 0:
                                fail("short Git object copy")
                            view = view[count:]
                        os.fchmod(out, 0o444)
                        os.fsync(out)
                    finally:
                        os.close(out)
                    records.append({"mode": "0444", "path": "pack/" + name, "sha256": sha256(data), "size": len(data)})
                for stem in pack_stems:
                    if stem + ".pack" not in names or stem + ".idx" not in names:
                        fail("Git pack lacks its matching index")

            os.mkdir("info", 0o755, dir_fd=dest_objects)
            info_dest = os.open("info", DIR_FLAGS, dir_fd=dest_objects)
            try:
                os.fchmod(info_dest, 0o755)
            finally:
                os.close(info_dest)
            pack_src = os.open("pack", DIR_FLAGS, dir_fd=source_objects)
            try:
                os.mkdir("pack", 0o755, dir_fd=dest_objects)
                pack_dst = os.open("pack", DIR_FLAGS, dir_fd=dest_objects)
                try:
                    os.fchmod(pack_dst, 0o755)
                    copy_pack_directory(pack_src, pack_dst)
                finally:
                    os.close(pack_dst)
            finally:
                os.close(pack_src)
        finally:
            os.close(dest_objects)
    finally:
        os.close(source_objects)
        os.close(dest_git)
    return sorted(records, key=lambda item: item["path"]), total


def parse_index(data):
    if len(data) < 32 or data[:4] != b"DIRC" or int.from_bytes(data[4:8], "big") != 2:
        fail("source Git index format is unsupported")
    if hashlib.sha1(data[:-20]).digest() != data[-20:]:
        fail("source Git index checksum is invalid")
    count = int.from_bytes(data[8:12], "big")
    if count > 100000:
        fail("source Git index entry count exceeds its limit")
    offset = 12
    entries = []
    seen = set()
    for _ in range(count):
        start = offset
        if offset + 62 > len(data) - 20:
            fail("source Git index entry is truncated")
        fixed = data[offset : offset + 62]
        mode_number = int.from_bytes(fixed[24:28], "big")
        oid = fixed[40:60].hex()
        flags = int.from_bytes(fixed[60:62], "big")
        stage = (flags >> 12) & 0x3
        extended = bool(flags & 0x4000)
        if stage != 0 or extended:
            fail("source Git index has conflict or extended entries")
        offset += 62
        name_length = flags & 0x0FFF
        if name_length < 0x0FFF:
            end = offset + name_length
            if end >= len(data) - 20 or data[end] != 0:
                fail("source Git index path is truncated")
            path_bytes = data[offset:end]
            offset = end + 1
        else:
            end = data.find(b"\0", offset, len(data) - 20)
            if end < 0:
                fail("source Git index path is not terminated")
            path_bytes = data[offset:end]
            offset = end + 1
        offset = start + ((offset - start + 7) // 8 * 8)
        if offset > len(data) - 20 or any(data[end + 1 : offset]):
            fail("source Git index has invalid entry padding")
        try:
            path = path_bytes.decode("utf-8", "strict")
        except UnicodeDecodeError:
            fail("source Git index path is not UTF-8")
        valid_relpath(path)
        if path in seen:
            fail("source Git index has duplicate paths")
        seen.add(path)
        mode = format(mode_number, "o")
        if mode not in ("100644", "100755", "120000", "160000"):
            fail("source Git index contains an unsupported mode")
        entries.append({"mode": mode, "oid": oid, "path": path})
    extension_names = []
    while offset < len(data) - 20:
        if offset + 8 > len(data) - 20:
            fail("source Git index extension header is truncated")
        signature = data[offset : offset + 4]
        size = int.from_bytes(data[offset + 4 : offset + 8], "big")
        offset += 8
        if signature != b"TREE" or extension_names or size > 1 << 20 or offset + size > len(data) - 20:
            fail("source Git index has unsupported extensions")
        extension_names.append(signature.decode("ascii"))
        offset += size
    if offset != len(data) - 20:
        fail("source Git index has trailing data")
    return entries, extension_names


def build_index(entries):
    ordered = sorted(entries, key=lambda item: item["path"].encode("utf-8"))
    output = bytearray(b"DIRC\x00\x00\x00\x02" + len(ordered).to_bytes(4, "big"))
    for item in ordered:
        path = item["path"].encode("utf-8", "strict")
        if len(path) > 0x0FFE:
            fail("source path exceeds the supported Git index length")
        start = len(output)
        fixed = bytearray(62)
        fixed[24:28] = int(item["mode"], 8).to_bytes(4, "big")
        fixed[40:60] = bytes.fromhex(item["oid"])
        fixed[60:62] = len(path).to_bytes(2, "big")
        output.extend(fixed)
        output.extend(path)
        output.append(0)
        output.extend(b"\0" * ((-(len(output) - start)) % 8))
    output.extend(hashlib.sha1(output).digest())
    return bytes(output)


def copy_index(
    source_git_fd,
    dest_root_fd,
    source_manifest,
    tree_entries,
    expected_index_sha=INDEX_SHA256,
    expected_observed_index_sha=OBSERVED_INDEX_SHA256,
):
    source_index = os.open("index", FILE_FLAGS, dir_fd=source_git_fd)
    try:
        st = os.fstat(source_index)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o644 or st.st_size > 16 << 20:
            fail("unsafe source Git index")
        data = read_fd(source_index, st.st_size, 16 << 20, "source Git index")
    finally:
        os.close(source_index)
    observed_sha = sha256(data)
    if observed_sha != expected_observed_index_sha:
        fail("source Git index differs from the independently recorded current index")
    if source_manifest.get("source_git_index_sha256") != expected_index_sha:
        fail("source manifest index digest changed")
    source_entries, extensions = parse_index(data)
    expected_entries = sorted(
        ({"mode": item["mode"], "oid": item.get("oid", item.get("commit")), "path": item["path"]} for item in tree_entries),
        key=lambda item: item["path"].encode("utf-8"),
    )
    if source_entries != expected_entries:
        fail("source Git index semantic entries differ from the pinned Git tree")
    index_data = build_index(expected_entries)
    git_fd = os.open(".git", DIR_FLAGS, dir_fd=dest_root_fd)
    try:
        write_file(git_fd, "index", index_data, 0o444)
    finally:
        os.close(git_fd)
    return {
        "manifest_sha256": expected_index_sha,
        "observed_source_sha256": observed_sha,
        "observed_source_size": st.st_size,
        "observed_source_mode": format(stat.S_IMODE(st.st_mode), "04o"),
        "observed_source_semantic_match": True,
        "source_extensions": extensions,
        "output_rebuilt_from_tree_sha256": sha256(index_data),
        "output_entry_count": len(expected_entries),
    }


def capsule_inventory(root_fd, expected_symlinks):
    files = []
    total_bytes = 0

    def walk(fd, prefix=""):
        nonlocal total_bytes
        try:
            names = sorted(os.listdir(fd), key=os.fsencode)
        except UnicodeError:
            fail("capsule contains a non-UTF-8 name")
        for name in names:
            if name in (".", "..") or "/" in name:
                fail("capsule contains an invalid entry name")
            path = posixpath.join(prefix, name) if prefix else name
            try:
                path.encode("utf-8", "strict")
            except UnicodeError:
                fail("capsule path is not valid UTF-8")
            st = os.stat(name, dir_fd=fd, follow_symlinks=False)
            owner = (st.st_uid, st.st_gid)
            if stat.S_ISDIR(st.st_mode):
                if owner != (0, 0) or stat.S_IMODE(st.st_mode) != 0o755:
                    fail("capsule directory ownership or mode changed: " + path)
                files.append({"gid": 0, "mode": "0755", "path": path, "type": "directory", "uid": 0})
                child = os.open(name, DIR_FLAGS, dir_fd=fd)
                try:
                    walk(child, path)
                finally:
                    os.close(child)
                continue
            if stat.S_ISREG(st.st_mode):
                if owner != (0, 0) or st.st_nlink != 1:
                    fail("capsule file ownership or link count changed: " + path)
                if path.startswith(".git/"):
                    allowed_modes = {0o444}
                    limit = MAX_OBJECTS
                else:
                    allowed_modes = {0o644, 0o755}
                    limit = MAX_BLOB
                mode = stat.S_IMODE(st.st_mode)
                if mode not in allowed_modes or st.st_size > limit:
                    fail("capsule file mode or size is invalid: " + path)
                file_fd = os.open(name, FILE_FLAGS, dir_fd=fd)
                try:
                    data = read_fd(file_fd, st.st_size, limit, "capsule file " + path)
                finally:
                    os.close(file_fd)
                total_bytes += len(data)
                if total_bytes > MAX_BLOBS_TOTAL + MAX_OBJECTS:
                    fail("capsule total file bytes exceed the limit")
                files.append({
                    "gid": 0,
                    "mode": format(mode, "04o"),
                    "path": path,
                    "sha256": sha256(data),
                    "size": len(data),
                    "type": "file",
                    "uid": 0,
                })
                continue
            if stat.S_ISLNK(st.st_mode):
                if owner != (0, 0) or st.st_nlink != 1 or path not in expected_symlinks:
                    fail("capsule has an unexpected symlink: " + path)
                target = os.readlink(name, dir_fd=fd)
                if target != expected_symlinks[path]:
                    fail("capsule symlink target changed: " + path)
                files.append({
                    "gid": 0,
                    "mode": "0777",
                    "path": path,
                    "target": target,
                    "type": "symlink",
                    "uid": 0,
                })
                continue
            fail("capsule contains a special filesystem entry: " + path)

    walk(root_fd)
    if len(files) > MAX_TREE_ENTRIES * 3:
        fail("capsule filesystem entry count exceeds the limit")
    files.sort(key=lambda item: item["path"].encode("utf-8"))
    encoded = json.dumps(files, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return files, sha256(encoded), total_bytes


def source_git_dir(source_fd, expected_config_sha=CONFIG_SHA256):
    git_fd = os.open(".git", DIR_FLAGS, dir_fd=source_fd)
    st = os.fstat(git_fd)
    if (st.st_uid, st.st_gid, stat.S_IMODE(st.st_mode)) != (0, 0, 0o700):
        os.close(git_fd)
        fail("source Git directory identity changed")
    try:
        config = os.open("config", FILE_FLAGS, dir_fd=git_fd)
        try:
            cst = os.fstat(config)
            if not stat.S_ISREG(cst.st_mode) or cst.st_nlink != 1 or (cst.st_uid, cst.st_gid) != (0, 0) or stat.S_IMODE(cst.st_mode) != 0o600:
                fail("source Git config file identity changed")
            data = read_fd(config, cst.st_size, 1 << 20, "source Git config")
            if sha256(data) != expected_config_sha:
                fail("source Git config differs from the reviewed materialization")
        finally:
            os.close(config)
        return git_fd
    except BaseException:
        os.close(git_fd)
        raise


def normalize_manifest_entries(manifest):
    raw_entries = manifest.get("entries")
    raw_gitlinks = manifest.get("gitlinks")
    if not isinstance(raw_entries, list) or not isinstance(raw_gitlinks, list):
        fail("source manifest entry lists are invalid")
    blobs = []
    links_in_entries = []
    for original in raw_entries:
        entry = dict(original)
        if entry.get("mode") == "160000" or entry.get("type") == "gitlink":
            oid = entry.get("oid")
            if entry.get("mode") != "160000" or entry.get("type") != "gitlink" or not isinstance(oid, str) or len(oid) != 40:
                fail("manifest gitlink entry is malformed")
            links_in_entries.append({"path": entry.get("path"), "commit": oid, "type": "gitlink"})
        else:
            if entry.get("type") != "blob" or entry.get("mode") not in ("100644", "100755", "120000"):
                fail("manifest blob entry is malformed")
            blobs.append(entry)
    links = []
    links_identity = []
    for original in raw_gitlinks:
        entry = dict(original)
        commit = entry.get("commit")
        links_identity.append({"path": entry.get("path"), "commit": commit, "type": "gitlink"})
        links.append({
            "path": entry.get("path"),
            "commit": commit,
            "mode": "160000",
            "oid": commit,
            "type": "gitlink",
        })
    links_in_entries.sort(key=lambda item: (str(item["path"]).encode("utf-8"), str(item["commit"])))
    links_identity.sort(key=lambda item: (str(item["path"]).encode("utf-8"), str(item["commit"])))
    links.sort(key=lambda item: (str(item["path"]).encode("utf-8"), str(item["commit"])))
    if links_in_entries != links_identity:
        fail("duplicated manifest gitlink representations disagree")
    return blobs, links, blobs + links


def materialize(
    source_path,
    manifest_bytes,
    destination_fd,
    expected_commit=COMMIT,
    expected_tree=TREE,
    expected_index_sha=INDEX_SHA256,
    expected_observed_index_sha=OBSERVED_INDEX_SHA256,
    expected_config_sha=CONFIG_SHA256,
):
    manifest = json.loads(manifest_bytes)
    if (manifest.get("schema"), manifest.get("commit"), manifest.get("tree"), manifest.get("repository")) != (1, expected_commit, expected_tree, REPOSITORY):
        fail("source manifest identity changed")
    if manifest.get("source_git_config_sha256") != expected_config_sha:
        fail("source Git config digest differs from the pinned input")
    entries, gitlinks, tree_entries = normalize_manifest_entries(manifest)
    if manifest.get("source_git_index_sha256") != expected_index_sha:
        fail("source manifest index digest differs from the pinned input")
    tree_root, seen, tree_oid = canonical_tree(entries, gitlinks)
    if tree_oid != expected_tree:
        fail("manifest entries do not reconstruct the pinned Git tree")
    source_fd = open_abs_dir(str(source_path))
    try:
        source_st = os.fstat(source_fd)
        if (source_st.st_uid, source_st.st_gid, stat.S_IMODE(source_st.st_mode)) != (0, 0, 0o755):
            fail("source root identity changed")
        expected_fs = set(seen)
        for entry in list(entries) + list(gitlinks):
            parts = valid_relpath(entry["path"])
            for stop in range(1, len(parts)):
                expected_fs.add("/".join(parts[:stop]))
        expected_fs.discard(".git")
        source_layout(source_fd, expected_fs)
        destination_st = os.fstat(destination_fd)
        if (destination_st.st_uid, destination_st.st_gid, stat.S_IMODE(destination_st.st_mode)) != (0, 0, 0o755):
            fail("destination root identity changed")
        if os.listdir(destination_fd):
            fail("destination root must be fresh and empty")
        blob_bytes = 0
        directory_paths = set()
        for path in seen:
            parts = valid_relpath(path)
            for stop in range(1, len(parts)):
                directory_paths.add("/".join(parts[:stop]))
        for directory in sorted(directory_paths, key=lambda value: (value.count("/"), value.encode("utf-8"))):
            mkdir_path(destination_fd, directory)
        for entry in sorted(entries, key=lambda value: value["path"].encode("utf-8")):
            path = entry["path"]
            if entry["mode"] == "120000":
                target = read_rel_symlink(source_fd, path)
                valid_symlink(path, target)
                target_bytes = target.encode("utf-8", "strict")
                if target != entry.get("symlink_target") or git_blob_oid(target_bytes) != entry["oid"]:
                    fail("source symlink differs from the Git blob")
                blob_bytes += len(target_bytes)
                write_symlink(destination_fd, path, target)
            else:
                mode = 0o755 if entry["mode"] == "100755" else 0o644
                data, st = read_rel_regular(source_fd, path, MAX_BLOB)
                if not stat.S_ISREG(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != mode or git_blob_oid(data) != entry["oid"]:
                    fail("source file differs from the Git blob: " + path)
                blob_bytes += len(data)
                write_file(destination_fd, path, data, mode)
            if blob_bytes > MAX_BLOBS_TOTAL:
                fail("source blobs exceed their total size limit")
        for gitlink in gitlinks:
            mkdir_path(destination_fd, gitlink["path"])

        source_git_fd = source_git_dir(source_fd, expected_config_sha)
        try:
            os.mkdir(".git", 0o755, dir_fd=destination_fd)
            git_out_fd = os.open(".git", DIR_FLAGS, dir_fd=destination_fd)
            try:
                os.fchmod(git_out_fd, 0o755)
                write_file(git_out_fd, "config", MINIMAL_GIT_CONFIG, 0o444)
                write_file(git_out_fd, "HEAD", b"ref: refs/heads/main\n", 0o444)
                mkdir_path(git_out_fd, "refs/heads")
                mkdir_path(git_out_fd, "refs/remotes/origin")
                refs_fd = os.open("refs/heads", DIR_FLAGS, dir_fd=git_out_fd)
                remote_fd = os.open("refs/remotes/origin", DIR_FLAGS, dir_fd=git_out_fd)
                try:
                    write_file(refs_fd, "main", (expected_commit + "\n").encode("ascii"), 0o444)
                    write_file(remote_fd, "main", (expected_commit + "\n").encode("ascii"), 0o444)
                    os.symlink("main", "HEAD", dir_fd=remote_fd)
                finally:
                    os.close(refs_fd)
                    os.close(remote_fd)
                mkdir_path(git_out_fd, "info")
                info_fd = os.open("info", DIR_FLAGS, dir_fd=git_out_fd)
                try:
                    write_file(info_fd, "exclude", b"", 0o444)
                finally:
                    os.close(info_fd)
                object_records, object_bytes = copy_objects(source_git_fd, destination_fd)
                index_record = copy_index(
                    source_git_fd,
                    destination_fd,
                    manifest,
                    tree_entries,
                    expected_index_sha,
                    expected_observed_index_sha,
                )
                # Mark this exact commit as shallow. The preview needs its tree and
                # index, not parent history or additional refs.
                write_file(git_out_fd, "shallow", (expected_commit + "\n").encode("ascii"), 0o444)
                git_inventory = {
                    "config_sha256": sha256(MINIMAL_GIT_CONFIG),
                    "head": "refs/heads/main",
                    "index": index_record,
                    "object_bytes": object_bytes,
                    "objects": object_records,
                    "origin": REPOSITORY,
                    "remote_head": "refs/remotes/origin/main",
                    "shallow_boundary": expected_commit,
                }
            finally:
                os.close(git_out_fd)
        finally:
            os.close(source_git_fd)
    finally:
        os.close(source_fd)
    return manifest, git_inventory


def fixture_snapshot(root):
    result = []

    def walk(fd, prefix=""):
        for name in sorted(os.listdir(fd), key=os.fsencode):
            path = posixpath.join(prefix, name) if prefix else name
            st = os.stat(name, dir_fd=fd, follow_symlinks=False)
            if stat.S_ISDIR(st.st_mode):
                child = os.open(name, DIR_FLAGS, dir_fd=fd)
                try:
                    walk(child, path)
                finally:
                    os.close(child)
                result.append((path, "dir", stat.S_IMODE(st.st_mode), st.st_uid, st.st_gid, st.st_nlink))
            elif stat.S_ISREG(st.st_mode):
                file_fd = os.open(name, FILE_FLAGS, dir_fd=fd)
                try:
                    data = read_fd(file_fd, st.st_size, MAX_OBJECTS, "fixture source file")
                finally:
                    os.close(file_fd)
                result.append((path, "file", stat.S_IMODE(st.st_mode), st.st_uid, st.st_gid, st.st_nlink, sha256(data)))
            elif stat.S_ISLNK(st.st_mode):
                result.append((path, "symlink", st.st_uid, st.st_gid, st.st_nlink, os.readlink(name, dir_fd=fd)))
            else:
                result.append((path, "special", stat.S_IFMT(st.st_mode)))

    root_fd = os.open(root, DIR_FLAGS)
    try:
        walk(root_fd)
    finally:
        os.close(root_fd)
    return sha256(json.dumps(result, separators=(",", ":")).encode())


def fixture_source(base, variant="valid"):
    source = base / "source"
    source.mkdir(mode=0o755)
    os.chmod(source, 0o755)
    files = {
        "README.md": ("100644", b"fixture source\n"),
        "AGENTS.md": ("100644", b"fixture role instructions\n"),
        "scripts/check.sh": ("100755", b"#!/bin/sh\nexit 0\n"),
    }
    entries = [
        {"mode": mode, "oid": git_blob_oid(data), "path": path, "type": "blob"}
        for path, (mode, data) in sorted(files.items())
    ]
    link_target = "AGENTS.md"
    entries.append({"mode": "120000", "oid": git_blob_oid(link_target.encode()), "path": "CLAUDE.md", "symlink_target": link_target, "type": "blob"})
    commit = "c" * 40
    link_path = "qualification/fixtures/submodule"
    gitlink = {"commit": "d" * 40, "content_materialized": False, "path": link_path}
    gitlink_entry = {"mode": "160000", "oid": gitlink["commit"], "path": link_path, "type": "gitlink"}
    entries.append(gitlink_entry)
    tree_root, _, tree = canonical_tree([entry for entry in entries if entry["type"] == "blob"], [{"path": link_path, "commit": gitlink["commit"]}])
    assert tree_root
    normalized = [
        {"mode": entry["mode"], "oid": entry.get("oid", entry.get("commit")), "path": entry["path"]}
        for entry in entries
    ]
    index_data = build_index(normalized)
    config = b"[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n"
    manifest = {
        "commit": commit,
        "entries": entries,
        "gitlinks": [gitlink],
        "host_tools": {
            "env": {"path": "/usr/bin/env", "sha256": "eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e"},
            "git": {"path": "/usr/bin/git", "sha256": "356db14e102d68a1a37d8a1ac577dfd678d45d46e92f468bef8b7154e7bfdc60", "version": "2.47.3"},
            "python": {"path": "/usr/bin/python3.13", "sha256": "889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9", "version": "3.13.5"},
        },
        "repository": REPOSITORY,
        "schema": 1,
        "source_git_config_sha256": sha256(config),
        "source_git_index_sha256": sha256(index_data),
        "tree": tree,
    }
    for path, (_, data) in files.items():
        target = source / path
        target.parent.mkdir(parents=True, exist_ok=True)
        os.chmod(target.parent, 0o755)
        target.write_bytes(data)
        os.chmod(target, 0o755 if files[path][0] == "100755" else 0o644)
    (source / "CLAUDE.md").symlink_to(link_target)
    gitlink_dir = source / link_path
    gitlink_dir.mkdir(parents=True)
    for directory in (source / "qualification", source / "qualification" / "fixtures"):
        os.chmod(directory, 0o755)
    os.chmod(gitlink_dir.parent, 0o755)
    os.chmod(gitlink_dir, 0o755)
    git_dir = source / ".git"
    git_dir.mkdir(mode=0o700)
    os.chmod(git_dir, 0o700)
    objects = git_dir / "objects"
    objects.mkdir(mode=0o700)
    os.chmod(objects, 0o700)
    info = objects / "info"
    pack = objects / "pack"
    info.mkdir(mode=0o700)
    pack.mkdir(mode=0o700)
    os.chmod(info, 0o700)
    os.chmod(pack, 0o700)
    for name, data in (("pack-fixture.pack", b"controlled fixture pack bytes"), ("pack-fixture.idx", b"controlled fixture index bytes")):
        item = pack / name
        item.write_bytes(data)
        os.chmod(item, 0o400)
    (git_dir / "config").write_bytes(config)
    os.chmod(git_dir / "config", 0o600)
    (git_dir / "index").write_bytes(index_data)
    os.chmod(git_dir / "index", 0o644)
    if variant == "extra":
        (source / "unexpected.txt").write_text("unexpected\n")
        os.chmod(source / "unexpected.txt", 0o644)
    elif variant == "bad_blob":
        (source / "README.md").write_text("changed\n")
        os.chmod(source / "README.md", 0o644)
    elif variant == "bad_index":
        wrong_index = build_index([{"mode": "100644", "oid": "e" * 40, "path": "wrong.txt"}])
        (git_dir / "index").write_bytes(wrong_index)
        os.chmod(git_dir / "index", 0o644)
        manifest["source_git_index_sha256"] = sha256(wrong_index)
    elif variant == "bad_config":
        (git_dir / "config").write_bytes(config + b"[include]\n\tpath = /etc/passwd\n")
        os.chmod(git_dir / "config", 0o600)
    elif variant == "semantic_index_drift":
        extension = b"TREE\x00\x00\x00\x00"
        changed_index = index_data[:-20] + extension
        changed_index += hashlib.sha1(changed_index).digest()
        (git_dir / "index").write_bytes(changed_index)
        os.chmod(git_dir / "index", 0o644)
    elif variant == "alternate":
        (info / "alternates").write_text("/untrusted/object-store\n")
        os.chmod(info / "alternates", 0o400)
    elif variant == "fifo":
        os.mkfifo(source / "pipe")
    return source, manifest, json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode(), commit, tree


def fixture_call(
    source,
    manifest_bytes,
    base,
    commit,
    tree,
    expected_index=None,
    expected_observed_index=None,
    expected_config=None,
    nonempty=False,
):
    destination = base / "output"
    destination.mkdir(mode=0o755)
    os.chmod(destination, 0o755)
    if nonempty:
        (destination / "sentinel").write_text("preserve\n")
        os.chmod(destination / "sentinel", 0o644)
    destination_fd = os.open(destination, DIR_FLAGS)
    try:
        return materialize(
            source,
            manifest_bytes,
            destination_fd,
            expected_commit=commit,
            expected_tree=tree,
            expected_index_sha=expected_index,
            expected_observed_index_sha=expected_observed_index if expected_observed_index is not None else expected_index,
            expected_config_sha=expected_config,
        )
    finally:
        os.close(destination_fd)


def fixture():
    import tempfile

    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"} or not sys.flags.isolated or not sys.flags.no_site or not sys.flags.dont_write_bytecode:
        fail("fixtures require the exact clean isolated Python environment")
    private_fd = open_abs_dir(str(PRIVATE))
    try:
        private_st = os.fstat(private_fd)
        if (private_st.st_uid, private_st.st_gid, stat.S_IMODE(private_st.st_mode)) != (0, 0, 0o700):
            fail("fixture parent is not private")
    finally:
        os.close(private_fd)
    passed = []
    with tempfile.TemporaryDirectory(prefix="input-fixture-v4-", dir=PRIVATE) as root:
        base = pathlib.Path(root)
        (base / "valid").mkdir(mode=0o700)
        source, manifest, data, commit, tree = fixture_source(base / "valid")
        before = fixture_snapshot(source)
        result, git_info = fixture_call(
            source, data, base / "valid", commit, tree,
            expected_index=manifest["source_git_index_sha256"],
            expected_config=manifest["source_git_config_sha256"],
        )
        output = base / "valid" / "output"
        assert result["tree"] == tree
        assert os.readlink(output / "CLAUDE.md") == "AGENTS.md"
        assert (output / "README.md").read_bytes() == b"fixture source\n"
        assert (output / ".git" / "config").read_bytes() == MINIMAL_GIT_CONFIG
        assert b"credential" not in (output / ".git" / "config").read_bytes().lower()
        assert (output / ".git" / "HEAD").read_bytes() == b"ref: refs/heads/main\n"
        assert (output / ".git" / "refs" / "heads" / "main").read_text().strip() == commit
        assert os.readlink(output / ".git" / "refs" / "remotes" / "origin" / "HEAD") == "main"
        assert git_info["index"]["observed_source_semantic_match"] is True
        assert stat.S_IMODE((output / ".git" / "index").stat().st_mode) == 0o444
        assert not (output / ".git" / "objects" / "info" / "alternates").exists()
        output_fd = os.open(output, DIR_FLAGS)
        try:
            allowed_symlinks = {
                item["path"]: item["symlink_target"]
                for item in result["entries"]
                if item.get("mode") == "120000"
            }
            allowed_symlinks[".git/refs/remotes/origin/HEAD"] = "main"
            capsule_files, capsule_digest, capsule_bytes = capsule_inventory(output_fd, allowed_symlinks)
        finally:
            os.close(output_fd)
        assert capsule_files and capsule_bytes > 0 and len(capsule_digest) == 64
        assert fixture_snapshot(source) == before
        passed.append("valid_raw_tree_sanitized_git_and_capsule_inventory")

        case = base / "semantic_index_drift"
        case.mkdir(mode=0o700)
        source, manifest, data, commit, tree = fixture_source(case, "semantic_index_drift")
        current_index = (source / ".git" / "index").read_bytes()
        current_index_sha = sha256(current_index)
        result, git_info = fixture_call(
            source,
            data,
            case,
            commit,
            tree,
            expected_index=manifest["source_git_index_sha256"],
            expected_observed_index=current_index_sha,
            expected_config=manifest["source_git_config_sha256"],
        )
        if (
            git_info["index"]["observed_source_sha256"] != current_index_sha
            or git_info["index"]["manifest_sha256"] != manifest["source_git_index_sha256"]
            or not git_info["index"]["observed_source_semantic_match"]
            or git_info["index"]["output_rebuilt_from_tree_sha256"] != sha256(build_index([
                {"mode": item["mode"], "oid": item.get("oid", item.get("commit")), "path": item["path"]}
                for item in manifest["entries"]
            ]))
        ):
            fail("semantic index revalidation did not preserve the manifest and rebuild from tree")
        wrong_observed = case / "wrong-observed-index"
        wrong_observed.mkdir(mode=0o700)
        try:
            fixture_call(
                source,
                data,
                wrong_observed,
                commit,
                tree,
                expected_index=manifest["source_git_index_sha256"],
                expected_observed_index=manifest["source_git_index_sha256"],
                expected_config=manifest["source_git_config_sha256"],
            )
        except InputError:
            passed.append("reject_unpinned_observed_index")
        else:
            fail("fixture accepted an unpinned observed source index")
        passed.append("accept_semantic_index_revalidation_and_rebuild")

        cases = ["extra", "bad_blob", "bad_index", "bad_config", "alternate", "fifo"]
        for variant in cases:
            case = base / variant
            case.mkdir(mode=0o700)
            source, manifest, data, commit, tree = fixture_source(case, variant)
            before = fixture_snapshot(source)
            rejected = False
            try:
                fixture_call(
                    source, data, case, commit, tree,
                    expected_index=manifest["source_git_index_sha256"],
                    expected_config=manifest["source_git_config_sha256"],
                )
            except InputError:
                rejected = True
            if not rejected or fixture_snapshot(source) != before:
                fail("fixture source was accepted or changed for " + variant)
            passed.append("reject_" + variant)

        case = base / "bad_symlink"
        case.mkdir(mode=0o700)
        source, manifest, data, commit, tree = fixture_source(case)
        bad = json.loads(data)
        link = next(item for item in bad["entries"] if item["mode"] == "120000")
        link["symlink_target"] = "../.git/config"
        bad_bytes = json.dumps(bad, sort_keys=True, separators=(",", ":")).encode()
        try:
            fixture_call(source, bad_bytes, case, commit, tree, expected_index=manifest["source_git_index_sha256"], expected_config=manifest["source_git_config_sha256"])
        except InputError:
            passed.append("reject_git_metadata_symlink")
        else:
            fail("fixture accepted Git metadata symlink")

        case = base / "nonempty_destination"
        case.mkdir(mode=0o700)
        source, manifest, data, commit, tree = fixture_source(case)
        try:
            fixture_call(source, data, case, commit, tree, expected_index=manifest["source_git_index_sha256"], expected_config=manifest["source_git_config_sha256"], nonempty=True)
        except InputError:
            if (case / "output" / "sentinel").read_text() != "preserve\n":
                fail("fixture overwrote a pre-existing destination entry")
            passed.append("reject_nonempty_destination_without_change")
        else:
            fail("fixture accepted a nonempty destination")
    verify_close_range_fixture()
    passed.append("close_inherited_fd_above_lowered_hard_limit")
    print(json.dumps({"status": "PASS", "cases": passed, "case_count": len(passed)}, sort_keys=True))


def close_range_fixture_child():
    import fcntl

    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"} or not sys.flags.isolated or not sys.flags.no_site or not sys.flags.dont_write_bytecode:
        fail("FD fixture requires the exact clean isolated environment")
    verify_python_runtime()
    hard = resource.getrlimit(resource.RLIMIT_NOFILE)[1]
    if hard != resource.RLIM_INFINITY and hard < 257:
        fail("FD fixture host hard limit is too small")
    source_fd = os.open("/dev/null", os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    target_fd = 256
    try:
        os.dup2(source_fd, target_fd, inheritable=True)
    finally:
        os.close(source_fd)
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    close_inherited_descriptors()
    try:
        fcntl.fcntl(target_fd, fcntl.F_GETFD)
    except OSError as exc:
        if exc.errno != 9:
            raise
    else:
        fail("close_range left a descriptor above the lowered hard limit open")
    print("closed fd 256")


def verify_close_range_fixture():
    import subprocess

    script_path = str(pathlib.Path(__file__).resolve())
    completed = subprocess.run(
        [sys.executable, "-I", "-S", "-B", script_path, "--fd-fixture"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
        close_fds=True,
        timeout=10,
        check=False,
    )
    if completed.returncode != 0 or completed.stdout != b"closed fd 256\n" or completed.stderr:
        fail("full-range inherited-FD fixture failed")


def stage_entry():
    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}:
        fail("run with the exact clean environment")
    if not sys.flags.isolated or not sys.flags.no_site or not sys.flags.dont_write_bytecode:
        fail("run with Python -I -S -B")
    verify_python_runtime()
    if (os.geteuid(), os.getegid(), os.getgroups()) != (0, 0, [0]):
        fail("staging requires only root identity and group")
    if os.uname().nodename.split(".", 1)[0] != "bastion":
        fail("unexpected host")
    _, hard_nofile = resource.getrlimit(resource.RLIMIT_NOFILE)
    if hard_nofile != resource.RLIM_INFINITY and hard_nofile < 64:
        fail("host file descriptor limit is below the required bounded limit")
    resource.setrlimit(resource.RLIMIT_CPU, (180, 180))
    resource.setrlimit(resource.RLIMIT_FSIZE, (128 << 20, 128 << 20))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_NPROC, (32, 32))
    resource.setrlimit(resource.RLIMIT_AS, (1 << 30, 1 << 30))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)
    task_fd = open_abs_dir(str(TASK))
    try:
        task_stat = os.fstat(task_fd)
        if (task_stat.st_uid, task_stat.st_gid, stat.S_IMODE(task_stat.st_mode)) != (0, 0, 0o700):
            fail("task directory is not root-owned mode0700")
        try:
            log_fd = os.open("materialize-input-86f-stage-v4.log", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=task_fd)
        except FileExistsError:
            fail("stage log already exists")
        log_stat = os.fstat(log_fd)
        if not stat.S_ISREG(log_stat.st_mode) or log_stat.st_nlink != 1 or (log_stat.st_uid, log_stat.st_gid) != (0, 0) or stat.S_IMODE(log_stat.st_mode) != 0o600:
            os.close(log_fd)
            fail("stage log is unsafe")
        null_fd = os.open("/dev/null", os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
        os.dup2(null_fd, 0)
        os.dup2(log_fd, 1)
        os.dup2(log_fd, 2)
        os.close(null_fd)
        os.close(log_fd)
        close_inherited_descriptors()
        private_fd = os.open(PRIVATE.name, DIR_FLAGS, dir_fd=task_fd)
        try:
            private_stat = os.fstat(private_fd)
            if (private_stat.st_uid, private_stat.st_gid, stat.S_IMODE(private_stat.st_mode)) != (0, 0, 0o700):
                fail("private preview directory is not root-owned mode0700")
            try:
                os.stat(OUTPUT_NAME, dir_fd=private_fd, follow_symlinks=False)
                fail("source output already exists")
            except FileNotFoundError:
                pass
            try:
                os.stat(RECORD_NAME, dir_fd=task_fd, follow_symlinks=False)
                fail("source record already exists")
            except FileNotFoundError:
                pass
            manifest_data = source_manifest_bytes(task_fd)
            source_evidence = verify_source_evidence(task_fd)
            parsed = json.loads(manifest_data)
            if (parsed.get("commit"), parsed.get("tree"), parsed.get("repository")) != (COMMIT, TREE, REPOSITORY):
                fail("source manifest does not match the pinned Velnor identity")
            if parsed.get("source_git_index_sha256") != INDEX_SHA256:
                fail("source manifest index digest differs from the frozen input")
            source_fd = open_abs_dir(str(SOURCE))
            try:
                source_st = os.fstat(source_fd)
                if (source_st.st_uid, source_st.st_gid, stat.S_IMODE(source_st.st_mode)) != (0, 0, 0o755):
                    fail("source directory identity changed")
            finally:
                os.close(source_fd)
            os.mkdir(OUTPUT_NAME, 0o755, dir_fd=private_fd)
            output_fd = os.open(OUTPUT_NAME, DIR_FLAGS, dir_fd=private_fd)
            try:
                os.fchmod(output_fd, 0o755)
                result_manifest, git_inventory = materialize(
                    SOURCE,
                    manifest_data,
                    output_fd,
                    expected_commit=COMMIT,
                    expected_tree=TREE,
                    expected_index_sha=INDEX_SHA256,
                    expected_config_sha=CONFIG_SHA256,
                )
                expected_paths = {item["path"] for item in result_manifest["entries"]}
                expected_paths.update(item["path"] for item in result_manifest["gitlinks"])
                for path in tuple(expected_paths):
                    parts = valid_relpath(path)
                    for stop in range(1, len(parts)):
                        expected_paths.add("/".join(parts[:stop]))
                source_layout(output_fd, expected_paths)
                expected_symlinks = {
                    item["path"]: item["symlink_target"]
                    for item in result_manifest["entries"]
                    if item.get("mode") == "120000"
                }
                expected_symlinks[".git/refs/remotes/origin/HEAD"] = "main"
                capsule_files, capsule_sha256, capsule_bytes = capsule_inventory(output_fd, expected_symlinks)
            finally:
                os.close(output_fd)
            record = {
                "schema": 3,
                "status": "PASS; private sanitized Git capsule staged from verified raw Git blobs",
                "source_commit": COMMIT,
                "source_tree": TREE,
                "source_manifest_sha256": SOURCE_MANIFEST_SHA256,
                "source_manifest_size": SOURCE_MANIFEST_SIZE,
                "repository": REPOSITORY,
                "source_fetch_evidence": source_evidence,
                "entry_count": len(result_manifest["entries"]),
                "gitlink_count": len(result_manifest["gitlinks"]),
                "symlink_count": sum(item["mode"] == "120000" for item in result_manifest["entries"]),
                "git": git_inventory,
                "capsule": {
                    "identity": {
                        "commit": COMMIT,
                        "tree": TREE,
                        "origin": REPOSITORY,
                        "head": "refs/heads/main",
                        "remote_head": "refs/remotes/origin/main",
                    },
                    "files_sha256": capsule_sha256,
                    "file_count": len(capsule_files),
                    "file_bytes": capsule_bytes,
                    "files": capsule_files,
                    "source_index": git_inventory["index"],
                },
                "destination": str(PRIVATE / OUTPUT_NAME),
                "destination_owner_mode": "root:root 0755; bind read-only into the preview namespace",
                "source_index_revalidation": git_inventory["index"],
                "scope": "raw Git blobs and sanitized local Git metadata; no source filters, hooks, credentials, source execution, Cargo, or network",
            }
            encoded = (json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n").encode()
            write_file(task_fd, RECORD_NAME, encoded, 0o600)
            print(json.dumps({
                "capsule_sha256": capsule_sha256,
                "record_sha256": sha256(encoded),
                "status": "PASS",
                "source_commit": COMMIT,
                "source_tree": TREE,
            }, sort_keys=True))
        finally:
            os.close(private_fd)
    finally:
        os.close(task_fd)


def stage():
    stage_entry()


def main():
    if sys.argv[1:] == ["--fixture"]:
        fixture()
    elif sys.argv[1:] == ["--fd-fixture"]:
        close_range_fixture_child()
    elif sys.argv[1:] == ["--stage"]:
        stage()
    else:
        raise SystemExit("usage: materialize_input_v4.py --fixture|--fd-fixture|--stage")


if __name__ == "__main__":
    try:
        main()
    except InputError as exc:
        raise SystemExit("generator preview input: " + str(exc))
