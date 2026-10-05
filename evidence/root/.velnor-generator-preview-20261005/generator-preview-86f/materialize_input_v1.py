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
import tempfile
import ctypes

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
OUTPUT_NAME = "input-86f-v1"
RECORD_NAME = "input-86f-v1-record.json"
MAX_BLOB = 64 << 20
MAX_OBJECTS = 64 << 20
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
    if not isinstance(raw, str) or not raw or raw.startswith("/") or "\\" in raw:
        fail("invalid repository path")
    parts = raw.split("/")
    if any(part in ("", ".", "..", ".git") for part in parts):
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


def canonical_tree(entries, gitlinks):
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


def source_manifest_bytes():
    fd = os.open(SOURCE_MANIFEST, FILE_FLAGS)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600 or st.st_nlink != 1 or st.st_size != SOURCE_MANIFEST_SIZE:
            fail("source manifest identity changed")
        data = os.read(fd, SOURCE_MANIFEST_SIZE + 1)
        if len(data) != SOURCE_MANIFEST_SIZE or sha256(data) != SOURCE_MANIFEST_SHA256:
            fail("source manifest digest changed")
        return data
    finally:
        os.close(fd)


def source_layout(root_fd, expected_paths):
    actual = set()

    def walk(fd, prefix):
        for name in os.listdir(fd):
            if name == ".git" and prefix == "":
                continue
            if name in (".", "..") or "/" in name:
                fail("invalid source directory entry")
            path = posixpath.join(prefix, name) if prefix else name
                    st = os.stat(name, dir_fd=fd, follow_symlinks=False)
            actual.add(path)
            if stat.S_ISDIR(st.st_mode):
                if (st.st_uid, st.st_gid, stat.S_IMODE(st.st_mode)) != (0, 0, 0o755):
                    fail("source directory identity changed")
                child = os.open(name, DIR_FLAGS, dir_fd=fd)
                try:
                    walk(child, path)
                finally:
                    os.close(child)
            elif stat.S_ISREG(st.st_mode) or stat.S_ISLNK(st.st_mode):
                if (st.st_uid, st.st_gid) != (0, 0):
                    fail("source file owner changed")
            else:
                fail("unsupported source filesystem entry")

    walk(root_fd, "")
    if actual != expected_paths:
        fail("materialized source path inventory differs from manifest")


def copy_objects(source_git_fd, dest_root_fd):
    source_objects = os.open("objects", DIR_FLAGS, dir_fd=source_git_fd)
    dest_git = os.open(".git", DIR_FLAGS, dir_fd=dest_root_fd)
    try:
        os.mkdir("objects", 0o755, dir_fd=dest_git)
        dest_objects = os.open("objects", DIR_FLAGS, dir_fd=dest_git)
        try:
            os.fchmod(dest_objects, 0o755)
            records = []
            total = 0

            def walk(src_fd, dst_fd, prefix):
                nonlocal total
                for name in sorted(os.listdir(src_fd), key=os.fsencode):
                    if name in (".", "..") or "/" in name:
                        fail("invalid Git object path")
                    rel = posixpath.join(prefix, name) if prefix else name
                    st = os.stat(name, dir_fd=src_fd, follow_symlinks=False)
                    if stat.S_ISDIR(st.st_mode):
                        if (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) & 0o022:
                            fail("writable Git object directory")
                        os.mkdir(name, 0o755, dir_fd=dst_fd)
                        src_child = os.open(name, DIR_FLAGS, dir_fd=src_fd)
                        dst_child = os.open(name, DIR_FLAGS, dir_fd=dst_fd)
                        try:
                            os.fchmod(dst_child, 0o755)
                            walk(src_child, dst_child, rel)
                        finally:
                            os.close(src_child)
                            os.close(dst_child)
                    elif stat.S_ISREG(st.st_mode) and st.st_nlink == 1 and st.st_size <= MAX_OBJECTS:
                        if (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) & 0o022:
                            fail("unsafe Git object file")
                        if rel in ("info/alternates", "info/http-alternates"):
                            fail("Git object alternates are not allowed")
                        src_file = os.open(name, FILE_FLAGS, dir_fd=src_fd)
                        try:
                            data = os.read(src_file, st.st_size + 1)
                        finally:
                            os.close(src_file)
                        if len(data) != st.st_size:
                            fail("Git object file changed while reading")
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
                        finally:
                            os.close(out)
                        records.append({"path": rel, "sha256": sha256(data), "size": len(data)})
                    else:
                        fail("unsupported Git object entry")

            walk(source_objects, dest_objects, "")
        finally:
            os.close(dest_objects)
    finally:
        os.close(source_objects)
        os.close(dest_git)
    return sorted(records, key=lambda item: item["path"]), total


def copy_index(source_git_fd, dest_root_fd, source_manifest, expected_index_sha=INDEX_SHA256):
    source_index = os.open("index", FILE_FLAGS, dir_fd=source_git_fd)
    try:
        st = os.fstat(source_index)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600 or st.st_size > 16 << 20:
            fail("unsafe source Git index")
        data = os.read(source_index, st.st_size + 1)
    finally:
        os.close(source_index)
    if len(data) != st.st_size or sha256(data) != expected_index_sha or source_manifest.get("source_git_index_sha256") != expected_index_sha:
        fail("source Git index identity changed")
    git_fd = os.open(".git", DIR_FLAGS, dir_fd=dest_root_fd)
    try:
        write_file(git_fd, "index", data, 0o444)
    finally:
        os.close(git_fd)
    return data


def source_git_dir(source_fd, expected_config_sha=CONFIG_SHA256):
    git_fd = os.open(".git", DIR_FLAGS, dir_fd=source_fd)
    st = os.fstat(git_fd)
    if (st.st_uid, st.st_gid, stat.S_IMODE(st.st_mode)) != (0, 0, 0o700):
        os.close(git_fd)
        fail("source Git directory identity changed")
    config = os.open("config", FILE_FLAGS, dir_fd=git_fd)
    try:
        cst = os.fstat(config)
        data = os.read(config, cst.st_size + 1)
        if not stat.S_ISREG(cst.st_mode) or cst.st_nlink != 1 or (cst.st_uid, cst.st_gid) != (0, 0) or stat.S_IMODE(cst.st_mode) != 0o600 or len(data) != cst.st_size or sha256(data) != expected_config_sha:
            fail("source Git config differs from the reviewed materialization")
    finally:
        os.close(config)
    return git_fd


def materialize(source_path, manifest_bytes, destination_path, expected_commit=COMMIT, expected_tree=TREE, expected_index_sha=INDEX_SHA256, expected_config_sha=CONFIG_SHA256):
    manifest = json.loads(manifest_bytes)
    if (manifest.get("schema"), manifest.get("commit"), manifest.get("tree"), manifest.get("repository")) != (1, expected_commit, expected_tree, REPOSITORY):
        fail("source manifest identity changed")
    if manifest.get("source_git_config_sha256") != expected_config_sha:
        fail("source Git config digest differs from the pinned input")
    entries, gitlinks = manifest.get("entries"), manifest.get("gitlinks")
    if not isinstance(entries, list) or not isinstance(gitlinks, list):
        fail("source manifest entry lists are invalid")
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
        destination_path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        if destination_path.exists() or destination_path.is_symlink():
            fail("destination already exists")
        os.mkdir(destination_path, 0o755)
        dest_fd = open_abs_dir(str(destination_path))
        try:
            os.fchmod(dest_fd, 0o755)
            directories = sorted(
                {"/".join(valid_relpath(path)[:n]) for path in seen | {entry["path"] for entry in gitlinks} for n in range(1, len(valid_relpath(path)))}
            )
            for directory in directories:
                mkdir_path(dest_fd, directory)
            for entry in sorted(entries, key=lambda value: value["path"]):
                path = entry["path"]
                if entry["mode"] == "120000":
                    target = read_rel_symlink(source_fd, path)
                    valid_symlink(path, target)
                    if target != entry.get("symlink_target") or git_blob_oid(target.encode("utf-8")) != entry["oid"]:
                        fail("source symlink differs from the Git blob")
                    write_symlink(dest_fd, path, target)
                else:
                    mode = 0o755 if entry["mode"] == "100755" else 0o644
                    data, st = read_rel_regular(source_fd, path, MAX_BLOB)
                    if not stat.S_ISREG(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != mode or git_blob_oid(data) != entry["oid"]:
                        fail("source file differs from the Git blob: " + path)
                    write_file(dest_fd, path, data, mode)
            for gitlink in gitlinks:
                mkdir_path(dest_fd, gitlink["path"])

            source_git_fd = source_git_dir(source_fd, expected_config_sha)
            try:
                os.mkdir(".git", 0o755, dir_fd=dest_fd)
                git_out_fd = os.open(".git", DIR_FLAGS, dir_fd=dest_fd)
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
                        os.symlink("refs/remotes/origin/main", "HEAD", dir_fd=remote_fd)
                    finally:
                        os.close(refs_fd)
                        os.close(remote_fd)
                    mkdir_path(git_out_fd, "info")
                    info_fd = os.open("info", DIR_FLAGS, dir_fd=git_out_fd)
                    write_file(info_fd, "exclude", b"", 0o444)
                    os.close(info_fd)
                    object_records, object_bytes = copy_objects(source_git_fd, dest_fd)
                    index_bytes = copy_index(source_git_fd, dest_fd, manifest, expected_index_sha)
                    shallow_fd = None
                    try:
                        shallow_fd = os.open("shallow", FILE_FLAGS, dir_fd=source_git_fd)
                    except FileNotFoundError:
                        pass
                    if shallow_fd is not None:
                        try:
                            shallow_st = os.fstat(shallow_fd)
                            shallow = os.read(shallow_fd, 4097)
                            if not stat.S_ISREG(shallow_st.st_mode) or len(shallow) != shallow_st.st_size or shallow_st.st_size > 4096:
                                fail("unsafe source shallow boundary")
                            if any(line.strip() != expected_commit.encode("ascii") for line in shallow.splitlines() if line.strip()):
                                fail("source shallow boundary differs from the exact commit")
                            write_file(git_out_fd, "shallow", shallow, 0o444)
                        finally:
                            os.close(shallow_fd)
                    git_inventory = {
                        "config_sha256": sha256(MINIMAL_GIT_CONFIG),
                    "head": "refs/heads/main",
                        "index_sha256": sha256(index_bytes),
                        "object_bytes": object_bytes,
                        "objects": object_records,
                        "origin": REPOSITORY,
                        "remote_head": "refs/remotes/origin/main",
                    }
                finally:
                    os.close(git_out_fd)
            finally:
                os.close(source_git_fd)
        finally:
            os.close(dest_fd)
    finally:
        os.close(source_fd)
    return manifest, git_inventory


def fixture():
    with tempfile.TemporaryDirectory(prefix="input-fixture-", dir=PRIVATE) as root:
        base = pathlib.Path(root)
        source = base / "source"
        source.mkdir(mode=0o700)
        regular = b"fixture source\n"
        symlink_target = "AGENTS.md"
        entries = [
            {"mode": "100644", "oid": git_blob_oid(regular), "path": "README.md", "type": "blob"},
            {"mode": "120000", "oid": git_blob_oid(symlink_target.encode()), "path": "CLAUDE.md", "symlink_target": symlink_target, "type": "blob"},
        ]
        gitlinks = [{"commit": "1" * 40, "content_materialized": False, "path": "qualification/submodule"}]
        tree_root, paths, oid = canonical_tree(entries, gitlinks)
        for name in ("README.md", "AGENTS.md"):
            target = source / name
            target.write_bytes(regular if name == "README.md" else b"agent rules\n")
        os.chmod(source / "README.md", 0o644)
        os.chmod(source / "AGENTS.md", 0o644)
        (source / "CLAUDE.md").symlink_to(symlink_target)
        (source / "qualification" / "submodule").mkdir(parents=True)
        (source / ".git").mkdir(mode=0o700)
        (source / ".git" / "objects").mkdir()
        (source / ".git" / "refs" / "heads").mkdir(parents=True)
        (source / ".git" / "refs" / "remotes" / "origin").mkdir(parents=True)
        (source / ".git" / "config").write_bytes(b"fixture config")
        (source / ".git" / "index").write_bytes(b"fixture index")
        manifest = {
            "schema": 1,
            "commit": COMMIT,
            "tree": oid,
            "repository": REPOSITORY,
            "entries": entries,
            "gitlinks": gitlinks,
        }
        # The stage path validates the production lock/index/config pins.
        # These fixture-only hooks cover tree hashing and path/symlink rejection.
        assert tree_root and paths
        assert git_blob_oid(regular) == entries[0]["oid"]
        assert git_blob_oid(symlink_target.encode()) == entries[1]["oid"]
        for bad in ["../escape", "/absolute", "a//b", ".git/config", "a\\b"]:
            try:
                valid_relpath(bad)
            except InputError:
                continue
            fail("fixture accepted unsafe path")
        for bad in ["/etc/passwd", "../../outside", "a\\b"]:
            try:
                valid_symlink("sub/CLAUDE.md", bad)
            except InputError:
                continue
            fail("fixture accepted unsafe symlink")
        print(json.dumps({"status": "PASS", "tree_reconstruction": True, "unsafe_paths_rejected": True, "unsafe_symlinks_rejected": True}, sort_keys=True))


def stage():
    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}:
        fail("run with the exact clean environment")
    if not sys.flags.isolated or not sys.flags.no_site or not sys.flags.dont_write_bytecode:
        fail("run with Python -I -S -B")
    if (os.geteuid(), os.getegid(), os.getgroups()) != (0, 0, [0]):
        fail("staging requires only root identity and group")
    if os.uname().nodename.split(".", 1)[0] != "bastion":
        fail("unexpected host")
    resource.setrlimit(resource.RLIMIT_CPU, (180, 180))
    resource.setrlimit(resource.RLIMIT_FSIZE, (128 << 20, 128 << 20))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_NPROC, (32, 32))
    resource.setrlimit(resource.RLIMIT_AS, (1 << 30, 1 << 30))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)
    private_fd = open_abs_dir(str(PRIVATE))
    try:
        if os.fstat(private_fd).st_mode & 0o077:
            fail("private preview directory is not mode0700")
        manifest_data = source_manifest_bytes()
        source_fd = open_abs_dir(str(SOURCE))
        try:
            st = os.fstat(source_fd)
            if (st.st_uid, st.st_gid, stat.S_IMODE(st.st_mode)) != (0, 0, 0o755):
                fail("source directory identity changed")
        finally:
            os.close(source_fd)
        destination = PRIVATE / OUTPUT_NAME
        if os.path.lexists(destination):
            fail("source output already exists")
        parsed = json.loads(manifest_data)
        # Re-check the exact source tree manifest before staging any output.
        if (parsed.get("commit"), parsed.get("tree"), parsed.get("repository")) != (COMMIT, TREE, REPOSITORY):
            fail("source manifest does not match the pinned Velnor identity")
        result_manifest, git_inventory = materialize(SOURCE, manifest_data, destination)
        record = {
            "schema": 1,
            "status": "PASS; private readable source input staged from verified Git blobs",
            "source_commit": COMMIT,
            "source_tree": TREE,
            "source_manifest_sha256": SOURCE_MANIFEST_SHA256,
            "source_manifest_size": SOURCE_MANIFEST_SIZE,
            "repository": REPOSITORY,
            "entry_count": len(result_manifest["entries"]),
            "gitlink_count": len(result_manifest["gitlinks"]),
            "symlink_count": sum(item["mode"] == "120000" for item in result_manifest["entries"]),
            "git": git_inventory,
            "destination": str(destination),
            "destination_owner_mode": "root:root 0755; input mount will be read-only",
            "scope": "no filters, hooks, credential helpers, source execution, Cargo, or network",
        }
        encoded = (json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n").encode()
        fd = os.open(RECORD_NAME, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=private_fd)
        try:
            os.write(fd, encoded)
            os.fsync(fd)
        finally:
            os.close(fd)
        print(json.dumps({"record_sha256": sha256(encoded), "status": "PASS", "source_tree": TREE}, sort_keys=True))
    finally:
        os.close(private_fd)


def main():
    if sys.argv[1:] == ["--fixture"]:
        fixture()
    elif sys.argv[1:] == ["--stage"]:
        stage()
    else:
        raise SystemExit("usage: materialize_input_v1.py --fixture|--stage")


if __name__ == "__main__":
    try:
        main()
    except InputError as exc:
        raise SystemExit("generator preview input: " + str(exc))
