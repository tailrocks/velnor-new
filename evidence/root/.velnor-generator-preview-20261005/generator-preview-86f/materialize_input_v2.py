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
OUTPUT_NAME = "input-86f-v2"
RECORD_NAME = "input-86f-v2-record.json"
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
                names = sorted(os.listdir(src_fd), key=os.fsencode)
                if not names or any(not name.endswith((".pack", ".idx", ".rev")) for name in names):
                    fail("unexpected Git pack file name")
                pack_stems = {name.rsplit(".", 1)[0] for name in names}
                if any(not stem.startswith("pack-") for stem in pack_stems):
                    fail("unexpected Git pack file prefix")
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


def copy_index(source_git_fd, dest_root_fd, source_manifest, tree_entries, expected_index_sha=INDEX_SHA256):
    source_index = os.open("index", FILE_FLAGS, dir_fd=source_git_fd)
    try:
        st = os.fstat(source_index)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o644 or st.st_size > 16 << 20:
            fail("unsafe source Git index")
        data = read_fd(source_index, st.st_size, 16 << 20, "source Git index")
    finally:
        os.close(source_index)
    observed_sha = sha256(data)
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
        "observed_source_semantic_match": True,
        "source_extensions": extensions,
        "output_rebuilt_from_tree_sha256": sha256(index_data),
        "output_entry_count": len(expected_entries),
    }


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
    for original in raw_gitlinks:
        entry = dict(original)
        links.append({"path": entry.get("path"), "commit": entry.get("commit"), "type": "gitlink"})
    links_in_entries.sort(key=lambda item: (str(item["path"]).encode("utf-8"), str(item["commit"])))
    links.sort(key=lambda item: (str(item["path"]).encode("utf-8"), str(item["commit"])))
    if links_in_entries != links:
        fail("duplicated manifest gitlink representations disagree")
    return blobs, links, blobs + links


def materialize(source_path, manifest_bytes, destination_fd, expected_commit=COMMIT, expected_tree=TREE, expected_index_sha=INDEX_SHA256, expected_config_sha=CONFIG_SHA256):
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
                index_record = copy_index(source_git_fd, destination_fd, manifest, tree_entries, expected_index_sha)
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
    elif variant == "alternate":
        (info / "alternates").write_text("/untrusted/object-store\n")
        os.chmod(info / "alternates", 0o400)
    elif variant == "fifo":
        os.mkfifo(source / "pipe")
    return source, manifest, json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode(), commit, tree


def fixture_call(source, manifest_bytes, base, commit, tree, expected_index=None, expected_config=None, nonempty=False):
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
            expected_config_sha=expected_config,
        )
    finally:
        os.close(destination_fd)


def fixture():
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
    with tempfile.TemporaryDirectory(prefix="input-fixture-v2-", dir=PRIVATE) as root:
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
        assert fixture_snapshot(source) == before
        passed.append("valid_raw_tree_and_sanitized_git")

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
    print(json.dumps({"status": "PASS", "cases": passed, "case_count": len(passed)}, sort_keys=True))


def stage_entry():
    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}:
        fail("run with the exact clean environment")
    if not sys.flags.isolated or not sys.flags.no_site or not sys.flags.dont_write_bytecode:
        fail("run with Python -I -S -B")
    if (os.geteuid(), os.getegid(), os.getgroups()) != (0, 0, [0]):
        fail("staging requires only root identity and group")
    if os.uname().nodename.split(".", 1)[0] != "bastion":
        fail("unexpected host")
    soft_nofile, hard_nofile = resource.getrlimit(resource.RLIMIT_NOFILE)
    if soft_nofile == resource.RLIM_INFINITY or soft_nofile > (1 << 20):
        fail("host file descriptor limit exceeds the reviewed close range")
    os.closerange(3, soft_nofile)
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
            log_fd = os.open("materialize-input-86f-stage-v2.log", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=task_fd)
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
            finally:
                os.close(output_fd)
            record = {
                "schema": 2,
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
                "destination": str(PRIVATE / OUTPUT_NAME),
                "destination_owner_mode": "root:root 0755; bind read-only into the preview namespace",
                "source_index_revalidation": git_inventory["index"],
                "scope": "raw Git blobs and sanitized local Git metadata; no source filters, hooks, credentials, source execution, Cargo, or network",
            }
            encoded = (json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n").encode()
            write_file(task_fd, RECORD_NAME, encoded, 0o600)
            print(json.dumps({"record_sha256": sha256(encoded), "status": "PASS", "source_tree": TREE}, sort_keys=True))
        finally:
            os.close(private_fd)
    finally:
        os.close(task_fd)


def stage():
    stage_entry()


def main():
    if sys.argv[1:] == ["--fixture"]:
        fixture()
    elif sys.argv[1:] == ["--stage"]:
        stage()
    else:
        raise SystemExit("usage: materialize_input_v2.py --fixture|--stage")


if __name__ == "__main__":
    try:
        main()
    except InputError as exc:
        raise SystemExit("generator preview input: " + str(exc))
