#!/usr/bin/env python3
"""Build and verify a lock-bound Cargo registry seed capsule.

This module intentionally uses only the Python standard library. It never
invokes Cargo, Rust, package build scripts, or registry/network clients.
"""

from __future__ import annotations

import hashlib
import gzip
import io
import json
import os
import re
import shutil
import stat
import tarfile
import time
import tomllib
import sys
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, BinaryIO, Callable, Iterable


SOURCE_COMMIT = "86f864aabc2192a9f8ccd3f01f2e25b7858c3f20"
SOURCE_TREE = "1e16a9270f3ea99b04e68521cfc876fa19db67ac"
LOCK_SHA256 = "95d251ccedbcc12095cecb89c9c08b14d72a5f0774d252029a559d45bb557b1a"
REGISTRY_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
LOCK_VERSION = 4
LOCK_PACKAGE_COUNT = 93
REGISTRY_PACKAGE_COUNT = 84
PATH_PACKAGE_COUNT = 9
REGISTRY_NAME_COUNT = 83
FORMAT = "velnor-cargo-seed-capsule-v3"

PRIVATE_ROOT = Path("/root/.velnor-generator-preview-20261005/cargo-seed-86f")
SOURCE_ROOT = Path("/root/.velnor-generator-preview-20261005/main-source-86f")
SOURCE_MANIFEST_PATH = Path("/root/.velnor-generator-preview-20261005/main-source-86f-manifest.json")
SOURCE_MANIFEST_SHA256 = "f12e0de7c5c22c5dc69e4f141d38d704a94f50494448dee3e896ea499a3e61b3"
PYTHON_PATH = Path("/usr/bin/python3.13")
PYTHON_SHA256 = "889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9"
GIT_PATH = Path("/usr/bin/git")
GIT_SHA256 = "356db14e102d68a1a37d8a1ac577dfd678d45d46e92f468bef8b7154e7bfdc60"
TASK_UID = 0
PLAN_OUTPUT = PRIVATE_ROOT / "seed-plan-v3.json"
CANDIDATE_OUTPUT = PRIVATE_ROOT / "candidates" / "86f-95d251cc"

DIR_OPEN_FLAGS = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
FILE_READ_FLAGS = (
    os.O_RDONLY
    | getattr(os, "O_NONBLOCK", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)

MAX_ARCHIVE_BYTES = 64 * 1024 * 1024
MAX_INDEX_RECORD_BYTES = 4 * 1024 * 1024
MAX_INDEX_CACHE_ENTRY_BYTES = 64 * 1024 * 1024
MAX_INDEX_CACHE_RECORDS = 100_000
MAX_LOCK_BYTES = 8 * 1024 * 1024
MAX_SOURCE_MANIFEST_BYTES = 4 * 1024 * 1024
MAX_CANDIDATE_MANIFEST_BYTES = 128 * 1024 * 1024
MAX_MANIFEST_HASH_BYTES = 256
MAX_PLAN_BYTES = 16 * 1024 * 1024
MAX_GIT_OUTPUT_BYTES = 1024 * 1024
GIT_TIMEOUT_SECONDS = 10
MAX_PACKAGE_UNPACKED_BYTES = 128 * 1024 * 1024
MAX_TOTAL_UNPACKED_BYTES = 512 * 1024 * 1024
MAX_PACKAGE_FILES = 20_000
MAX_PACKAGE_TAR_BYTES = MAX_PACKAGE_UNPACKED_BYTES + MAX_PACKAGE_FILES * 2048 + 1024 * 1024
MAX_SOURCE_MANIFEST_BYTES = 4 * 1024 * 1024
MAX_SOURCE_FILE_BYTES = 256 * 1024 * 1024
MAX_TOTAL_SOURCE_BYTES = 2 * 1024 * 1024 * 1024
SOURCE_FILE_COUNT = 1599
SOURCE_GITLINK_COUNT = 1
SOURCE_REPOSITORY = "https://github.com/tailrocks/velnor-new.git"
SOURCE_SCOPE = "raw tracked Git blobs; no checkout filters or hooks executed"
MAX_SOURCE_PATH_BYTES = 4096
MAX_SOURCE_PATH_DEPTH = 64

CRATE_NAME = re.compile(r"^[a-z0-9][a-z0-9_-]*$")
CRATE_VERSION = re.compile(r"^[0-9][0-9A-Za-z.+-]*$")
SHA256 = re.compile(r"^[a-f0-9]{64}$")


class SeedError(ValueError):
    """An input does not satisfy the pinned seed contract."""


@dataclass(frozen=True)
class SourcePin:
    commit: str
    tree: str
    lock_sha256: str
    source_manifest_sha256: str = SOURCE_MANIFEST_SHA256
    source_entry_count: int = SOURCE_FILE_COUNT
    source_gitlink_count: int = SOURCE_GITLINK_COUNT
    source_repository: str = SOURCE_REPOSITORY
    lock_version: int = LOCK_VERSION
    package_count: int = LOCK_PACKAGE_COUNT
    registry_package_count: int = REGISTRY_PACKAGE_COUNT
    path_package_count: int = PATH_PACKAGE_COUNT
    registry_name_count: int = REGISTRY_NAME_COUNT


EXACT_SOURCE = SourcePin(
    commit=SOURCE_COMMIT,
    tree=SOURCE_TREE,
    lock_sha256=LOCK_SHA256,
)


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _check_trusted_directory(descriptor: int, label: str) -> os.stat_result:
    info = os.fstat(descriptor)
    if not stat.S_ISDIR(info.st_mode):
        raise SeedError(f"expected a directory in trusted path: {label}")
    if info.st_uid != TASK_UID or info.st_mode & 0o022:
        raise SeedError(f"untrusted directory owner or writable permissions: {label}")
    return info


def _open_directory_at(parent: int, name: str, label: str) -> int:
    if not name or name in (".", "..") or "/" in name or "\\" in name:
        raise SeedError(f"unsafe directory component: {name!r}")
    try:
        descriptor = os.open(name, DIR_OPEN_FLAGS, dir_fd=parent)
    except OSError as exc:
        raise SeedError(f"cannot open nofollow trusted directory: {label}") from exc
    try:
        _check_trusted_directory(descriptor, label)
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def _open_trusted_directory(path: Path) -> int:
    path = Path(path)
    if not path.is_absolute():
        raise SeedError("trusted directory path must be absolute")
    parts = path.parts
    if not parts or parts[0] != "/" or any(part in (".", "..") for part in parts[1:]):
        raise SeedError(f"unsafe trusted directory path: {path}")
    descriptor = os.open("/", DIR_OPEN_FLAGS)
    try:
        _check_trusted_directory(descriptor, "/")
        for part in parts[1:]:
            child = _open_directory_at(descriptor, part, str(path))
            os.close(descriptor)
            descriptor = child
        result = descriptor
        descriptor = -1
        return result
    finally:
        if descriptor >= 0:
            os.close(descriptor)


def _open_file_at(parent: int, name: str, label: str) -> tuple[BinaryIO, os.stat_result]:
    if not name or name in (".", "..") or "/" in name or "\\" in name:
        raise SeedError(f"unsafe file component: {name!r}")
    try:
        descriptor = os.open(name, FILE_READ_FLAGS, dir_fd=parent)
    except OSError as exc:
        raise SeedError(f"cannot open nofollow regular file: {label}") from exc
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode):
            raise SeedError(f"expected a regular file: {label}")
        if info.st_uid != TASK_UID or info.st_mode & 0o022 or info.st_nlink != 1:
            raise SeedError(f"untrusted file owner, link count, or writable permissions: {label}")
        return os.fdopen(descriptor, "rb"), info
    except BaseException:
        os.close(descriptor)
        raise


def _open_relative_file(
    root_fd: int, relative: PurePosixPath, label: str
) -> tuple[BinaryIO, os.stat_result]:
    if relative.is_absolute() or not relative.parts or any(part in ("", ".", "..") for part in relative.parts):
        raise SeedError(f"unsafe relative file path: {relative}")
    current = os.dup(root_fd)
    try:
        for part in relative.parts[:-1]:
            child = _open_directory_at(current, part, f"{label}/{part}")
            os.close(current)
            current = child
        return _open_file_at(current, relative.parts[-1], label)
    finally:
        os.close(current)


def _relative_parts_under(path: Path, root: Path) -> tuple[str, ...]:
    path = Path(path)
    root = Path(root)
    if not path.is_absolute() or not root.is_absolute():
        raise SeedError("task paths must be absolute")
    try:
        relative = path.relative_to(root)
    except ValueError as exc:
        raise SeedError("path is outside the assigned task root") from exc
    parts = relative.parts
    if not parts or any(part in ("", ".", "..") or "/" in part or "\\" in part for part in parts):
        raise SeedError("unsafe task-relative path")
    return parts


def _open_directory_chain(root_fd: int, parts: tuple[str, ...], *, create: bool) -> int:
    current = os.dup(root_fd)
    try:
        for part in parts:
            try:
                child = _open_directory_at(current, part, f"task/{part}")
            except SeedError:
                if not create:
                    raise
                try:
                    os.mkdir(part, 0o755, dir_fd=current)
                except FileExistsError:
                    pass
                child = _open_directory_at(current, part, f"task/{part}")
            os.close(current)
            current = child
        result = current
        current = -1
        return result
    finally:
        if current >= 0:
            os.close(current)


def _open_existing_directory_under(root_fd: int, path: Path, root: Path) -> tuple[int, tuple[str, ...]]:
    parts = _relative_parts_under(path, root)
    descriptor = _open_directory_chain(root_fd, parts, create=False)
    return descriptor, parts


def _open_new_regular_at(parent: int, name: str) -> tuple[BinaryIO, tuple[int, int]]:
    if not name or name in (".", "..") or "/" in name or "\\" in name:
        raise SeedError(f"unsafe output filename: {name!r}")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0)
    descriptor = os.open(name, flags, 0o666, dir_fd=parent)
    info = os.fstat(descriptor)
    if not stat.S_ISREG(info.st_mode) or info.st_uid != TASK_UID or info.st_nlink != 1:
        os.close(descriptor)
        raise SeedError(f"unsafe newly created output file: {name}")
    return os.fdopen(descriptor, "wb"), (info.st_dev, info.st_ino)


def _unlink_created_at(parent: int, name: str, identity: tuple[int, int] | None) -> None:
    if identity is None:
        return
    try:
        current = os.stat(name, dir_fd=parent, follow_symlinks=False)
    except FileNotFoundError:
        return
    if stat.S_ISREG(current.st_mode) and (current.st_dev, current.st_ino) == identity:
        os.unlink(name, dir_fd=parent)


def _remove_tree_at(parent: int, name: str) -> None:
    try:
        shutil.rmtree(name, dir_fd=parent)
    except FileNotFoundError:
        pass


def _read_trusted_absolute(path: Path, max_bytes: int) -> bytes:
    parent_fd = _open_trusted_directory(path.parent)
    try:
        stream, _info = _open_file_at(parent_fd, path.name, str(path))
        with stream:
            return _read_open_file(stream, path, max_bytes)
    finally:
        os.close(parent_fd)


def _verify_pinned_tool(path: Path, expected_sha256: str) -> None:
    parent_fd = _open_trusted_directory(path.parent)
    try:
        stream, _info = _open_file_at(parent_fd, path.name, str(path))
        digest = hashlib.sha256()
        with stream:
            _consume_bounded(stream, path, 256 * 1024 * 1024, digest.update)
        if digest.hexdigest() != expected_sha256:
            raise SeedError(f"pinned host tool hash mismatch: {path}")
    finally:
        os.close(parent_fd)


def verify_host_identity() -> None:
    if os.geteuid() != TASK_UID:
        raise SeedError("v3 seed entry must run as the pinned root task identity")
    if Path(os.path.abspath(sys.executable)) != PYTHON_PATH or sys.version_info[:3] != (3, 13, 5):
        raise SeedError("Python runtime path or version does not match the frozen host pin")
    _verify_pinned_tool(PYTHON_PATH, PYTHON_SHA256)
    _verify_pinned_tool(GIT_PATH, GIT_SHA256)


def _read_open_file_for_relative(
    root_fd: int, relative: PurePosixPath, root_path: Path, max_bytes: int
) -> bytes:
    stream, _info = _open_relative_file(root_fd, relative, (root_path / relative.as_posix()).as_posix())
    with stream:
        return _read_open_file(stream, root_path / relative.as_posix(), max_bytes)


def _open_regular_binary(path: Path):
    # O_NONBLOCK is essential for the type check: opening a FIFO with O_RDONLY
    # can otherwise wait forever before fstat gets a chance to reject it.
    flags = (
        os.O_RDONLY
        | getattr(os, "O_NONBLOCK", 0)
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0)
    )
    try:
        descriptor = os.open(path, flags)
    except OSError as exc:
        raise SeedError(f"cannot open regular file: {path}") from exc
    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            raise SeedError(f"expected a regular file: {path}")
        return os.fdopen(descriptor, "rb")
    except BaseException:
        os.close(descriptor)
        raise


def _file_snapshot(info: os.stat_result) -> tuple[int, int, int, int, int, int]:
    return (
        info.st_dev,
        info.st_ino,
        info.st_size,
        info.st_mtime_ns,
        info.st_ctime_ns,
        info.st_mode,
    )


def _consume_bounded(
    source: BinaryIO,
    path: Path,
    max_bytes: int,
    consume: Callable[[bytes], None],
) -> int:
    if max_bytes < 0:
        raise SeedError("file size limit must be nonnegative")
    before = os.fstat(source.fileno())
    if not stat.S_ISREG(before.st_mode):
        raise SeedError(f"expected a regular file: {path}")
    if before.st_size > max_bytes:
        raise SeedError(f"file exceeds size limit: {path}")
    total = 0
    while True:
        # Ask for one byte beyond the remaining allowance so growth cannot be
        # mistaken for EOF at the configured limit.
        chunk = source.read(min(1024 * 1024, max_bytes - total + 1))
        if not chunk:
            break
        total += len(chunk)
        if total > max_bytes:
            raise SeedError(f"file grew beyond size limit while reading: {path}")
        consume(chunk)
    after = os.fstat(source.fileno())
    if _file_snapshot(before) != _file_snapshot(after) or total != before.st_size:
        raise SeedError(f"file changed while reading: {path}")
    return total


def _sha256_file(path: Path, *, max_bytes: int = MAX_PACKAGE_UNPACKED_BYTES) -> str:
    digest = hashlib.sha256()
    with _open_regular_binary(path) as source:
        _consume_bounded(source, path, max_bytes, digest.update)
    return digest.hexdigest()


def _read_open_file(source: BinaryIO, path: Path, max_bytes: int) -> bytes:
    pieces: list[bytes] = []
    _consume_bounded(source, path, max_bytes, pieces.append)
    return b"".join(pieces)


def _read_regular_file(path: Path, *, max_bytes: int) -> bytes:
    with _open_regular_binary(path) as source:
        return _read_open_file(source, path, max_bytes)


def _open_new_regular_binary(path: Path) -> tuple[BinaryIO, tuple[int, int]]:
    flags = (
        os.O_WRONLY
        | os.O_CREAT
        | os.O_EXCL
        | getattr(os, "O_NOFOLLOW", 0)
        | getattr(os, "O_CLOEXEC", 0)
    )
    descriptor = os.open(path, flags, 0o666)
    info = os.fstat(descriptor)
    identity = (info.st_dev, info.st_ino)
    return os.fdopen(descriptor, "wb"), identity


def _unlink_created_regular(path: Path, identity: tuple[int, int] | None) -> None:
    if identity is None:
        return
    try:
        current = path.lstat()
    except FileNotFoundError:
        return
    if stat.S_ISREG(current.st_mode) and (current.st_dev, current.st_ino) == identity:
        path.unlink()


def _inside(path: Path, root: Path) -> bool:
    try:
        path.resolve(strict=False).relative_to(root.resolve(strict=True))
    except (ValueError, FileNotFoundError):
        return False
    return True


def _safe_relative(raw: str) -> PurePosixPath:
    if not raw or "\\" in raw or raw.startswith("/"):
        raise SeedError(f"unsafe archive/index path: {raw!r}")
    pieces = raw.split("/")
    if any(part in ("", ".", "..") for part in pieces):
        raise SeedError(f"unsafe archive/index path: {raw!r}")
    return PurePosixPath(*pieces)


def index_relative_path(name: str) -> PurePosixPath:
    if not CRATE_NAME.fullmatch(name):
        raise SeedError(f"invalid registry crate name: {name!r}")
    if len(name) == 1:
        return PurePosixPath("1", name)
    if len(name) == 2:
        return PurePosixPath("2", name)
    if len(name) == 3:
        return PurePosixPath("3", name[0], name)
    return PurePosixPath(name[:2], name[2:4], name)


def _git_blob_oid_from_bytes(data: bytes) -> str:
    return hashlib.sha1(b"blob " + str(len(data)).encode("ascii") + b"\0" + data).hexdigest()


def _tree_oid_from_manifest(entries: list[dict[str, Any]]) -> str:
    root: dict[str, Any] = {}
    for entry in entries:
        relative = _safe_relative(entry["path"])
        if len(os.fsencode(relative.as_posix())) > MAX_SOURCE_PATH_BYTES or len(relative.parts) > MAX_SOURCE_PATH_DEPTH:
            raise SeedError("source manifest path exceeds structural bounds")
        if relative.parts[0] == ".git":
            raise SeedError("source manifest must not include Git administrative files")
        node = root
        for part in relative.parts[:-1]:
            existing = node.get(part)
            if existing is None:
                existing = {}
                node[part] = existing
            if not isinstance(existing, dict):
                raise SeedError(f"source tree path collision: {relative}")
            node = existing
        leaf = relative.parts[-1]
        if leaf in node:
            raise SeedError(f"duplicate source manifest path: {relative}")
        node[leaf] = (entry["mode"], entry["oid"])

    def tree_oid(node: dict[str, Any]) -> bytes:
        rows: list[tuple[bytes, bytes]] = []
        for name, value in node.items():
            if isinstance(value, dict):
                mode = b"40000"
                oid = tree_oid(value)
                sort_name = os.fsencode(name) + b"/"
            else:
                mode = value[0].encode("ascii")
                oid = bytes.fromhex(value[1])
                sort_name = os.fsencode(name) + b"\0"
            rows.append((sort_name, mode + b" " + os.fsencode(name) + b"\0" + oid))
        body = b"".join(row for _key, row in sorted(rows, key=lambda item: item[0]))
        return hashlib.sha1(b"tree " + str(len(body)).encode("ascii") + b"\0" + body).digest()

    return tree_oid(root).hex()


def _parse_source_manifest(raw: bytes, pin: SourcePin) -> dict[str, Any]:
    if len(raw) > MAX_SOURCE_MANIFEST_BYTES or _sha256_bytes(raw) != pin.source_manifest_sha256:
        raise SeedError("frozen raw source manifest hash or size mismatch")
    try:
        manifest = json.loads(raw)
    except (json.JSONDecodeError, UnicodeDecodeError, RecursionError) as exc:
        raise SeedError("frozen source manifest is invalid JSON") from exc
    if not isinstance(manifest, dict) or set(manifest) != {
        "schema",
        "repository",
        "commit",
        "tree",
        "scope",
        "source_git_config_sha256",
        "source_git_index_sha256",
        "entries",
        "gitlinks",
        "lfs_pointers",
        "host_tools",
    }:
        raise SeedError("frozen source manifest has an unsupported shape")
    if (
        manifest.get("schema") != 1
        or manifest.get("repository") != pin.source_repository
        or manifest.get("commit") != pin.commit
        or manifest.get("tree") != pin.tree
        or manifest.get("scope") != SOURCE_SCOPE
    ):
        raise SeedError("frozen source manifest identity mismatch")
    for key in ("source_git_config_sha256", "source_git_index_sha256"):
        if not isinstance(manifest.get(key), str) or not SHA256.fullmatch(manifest[key]):
            raise SeedError(f"frozen source manifest has invalid {key}")
    entries = manifest.get("entries")
    gitlinks = manifest.get("gitlinks")
    lfs_pointers = manifest.get("lfs_pointers")
    if (
        not isinstance(entries, list)
        or len(entries) != pin.source_entry_count
        or not isinstance(gitlinks, list)
        or len(gitlinks) != pin.source_gitlink_count
        or not isinstance(lfs_pointers, list)
    ):
        raise SeedError("frozen source manifest counts are invalid")
    gitlink_entries: dict[str, dict[str, Any]] = {}
    for entry in entries:
        if not isinstance(entry, dict) or not {"mode", "oid", "path", "type"}.issubset(entry):
            raise SeedError("frozen source manifest entry has an unsupported shape")
        path = entry.get("path")
        mode = entry.get("mode")
        oid = entry.get("oid")
        if not isinstance(path, str) or not isinstance(oid, str) or not re.fullmatch(r"[a-f0-9]{40}", oid):
            raise SeedError("frozen source manifest entry has an invalid path or object ID")
        if mode not in ("100644", "100755", "120000", "160000"):
            raise SeedError("frozen source manifest has an unsupported Git mode")
        allowed_keys = {"mode", "oid", "path", "type"}
        if mode == "120000":
            allowed_keys.add("symlink_target")
            if not isinstance(entry.get("symlink_target"), str):
                raise SeedError("frozen source symlink target is invalid")
        if set(entry) != allowed_keys:
            raise SeedError("frozen source manifest entry has unexpected fields")
        if entry.get("type") == "gitlink":
            if mode != "160000":
                raise SeedError("frozen source gitlink has an invalid mode")
            gitlink_entries[path] = entry
        elif entry.get("type") != "blob" or mode == "160000":
            raise SeedError("frozen source blob has an invalid type or mode")
    if len(gitlink_entries) != SOURCE_GITLINK_COUNT:
        raise SeedError("frozen source manifest gitlink count mismatch")
    for link in gitlinks:
        if (
            not isinstance(link, dict)
            or set(link) != {"commit", "content_materialized", "path"}
            or link.get("content_materialized") is not False
            or link.get("path") not in gitlink_entries
            or link.get("commit") != gitlink_entries[link["path"]].get("oid")
        ):
            raise SeedError("frozen source gitlink metadata mismatch")
    host_tools = manifest.get("host_tools")
    if host_tools != {
        "env": {
            "path": "/usr/bin/env",
            "sha256": "eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e",
        },
        "git": {"path": str(GIT_PATH), "sha256": GIT_SHA256, "version": "2.47.3"},
        "python": {"path": str(PYTHON_PATH), "sha256": PYTHON_SHA256, "version": "3.13.5"},
    }:
        raise SeedError("frozen source manifest host tool pins mismatch")
    if _tree_oid_from_manifest(entries) != pin.tree:
        raise SeedError("frozen source entries do not reconstruct the pinned Git tree")
    return manifest


def _git_blob_oid_from_file(source: BinaryIO, label: Path) -> tuple[str, int]:
    before = os.fstat(source.fileno())
    if before.st_size > MAX_SOURCE_FILE_BYTES:
        raise SeedError(f"tracked source file exceeds size limit: {label}")
    digest = hashlib.sha1()
    digest.update(b"blob " + str(before.st_size).encode("ascii") + b"\0")
    size = _consume_bounded(source, label, MAX_SOURCE_FILE_BYTES, digest.update)
    return digest.hexdigest(), size


def _check_source_tree(source_fd: int, manifest: dict[str, Any], source_root: Path) -> int:
    entries = {entry["path"]: entry for entry in manifest["entries"] if entry["type"] == "blob"}
    gitlinks = {entry["path"]: entry for entry in manifest["gitlinks"]}
    expected_dirs: set[str] = set()
    for relative_text in [*entries, *gitlinks]:
        relative = _safe_relative(relative_text)
        for depth in range(1, len(relative.parts)):
            expected_dirs.add(PurePosixPath(*relative.parts[:depth]).as_posix())
    expected_dirs.update(gitlinks)
    seen_files: set[str] = set()
    seen_dirs: set[str] = set()
    seen_gitlinks: set[str] = set()
    total_bytes = 0

    def visit(directory_fd: int, prefix: str) -> None:
        nonlocal total_bytes
        for name in os.listdir(directory_fd):
            if not prefix and name == ".git":
                continue
            relative_text = f"{prefix}/{name}" if prefix else name
            if relative_text in gitlinks:
                info = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                if not stat.S_ISDIR(info.st_mode) or info.st_uid != TASK_UID or info.st_mode & 0o022:
                    raise SeedError(f"source gitlink path is not a trusted directory: {relative_text}")
                link_fd = _open_directory_at(directory_fd, name, relative_text)
                try:
                    if os.listdir(link_fd):
                        raise SeedError(f"unmaterialized source gitlink is not empty: {relative_text}")
                finally:
                    os.close(link_fd)
                seen_gitlinks.add(relative_text)
                seen_dirs.add(relative_text)
                continue
            expected = entries.get(relative_text)
            info = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
            if expected is not None:
                if expected["mode"] == "120000":
                    if not stat.S_ISLNK(info.st_mode) or info.st_uid != TASK_UID or info.st_nlink != 1:
                        raise SeedError(f"source symlink does not match raw manifest: {relative_text}")
                    target_text = os.readlink(name, dir_fd=directory_fd)
                    if target_text != expected.get("symlink_target"):
                        raise SeedError(f"source symlink target mismatch: {relative_text}")
                    target = os.fsencode(target_text)
                    if len(target) > MAX_SOURCE_FILE_BYTES or _git_blob_oid_from_bytes(target) != expected["oid"]:
                        raise SeedError(f"source symlink blob mismatch: {relative_text}")
                    total_bytes += len(target)
                else:
                    if not stat.S_ISREG(info.st_mode):
                        raise SeedError(f"source blob is not a regular file: {relative_text}")
                    stream, opened = _open_file_at(directory_fd, name, relative_text)
                    try:
                        executable = bool(stat.S_IMODE(opened.st_mode) & 0o111)
                        if executable != (expected["mode"] == "100755"):
                            raise SeedError(f"source executable mode mismatch: {relative_text}")
                        actual_oid, file_size = _git_blob_oid_from_file(stream, source_root / relative_text)
                    finally:
                        stream.close()
                    total_bytes += file_size
                    if actual_oid != expected["oid"]:
                        raise SeedError(f"raw source blob mismatch: {relative_text}")
                if total_bytes > MAX_TOTAL_SOURCE_BYTES:
                    raise SeedError("raw source tree exceeds total byte limit")
                seen_files.add(relative_text)
                continue
            if relative_text not in expected_dirs or not stat.S_ISDIR(info.st_mode):
                raise SeedError(f"source has unmanifested or wrong-type path: {relative_text}")
            child_fd = _open_directory_at(directory_fd, name, relative_text)
            seen_dirs.add(relative_text)
            try:
                visit(child_fd, relative_text)
            finally:
                os.close(child_fd)

    visit(source_fd, "")
    missing_files = set(entries) - seen_files
    missing_dirs = expected_dirs - seen_dirs
    allowed_absent_gitlinks = {
        path for path, link in gitlinks.items() if link.get("content_materialized") is False
    }
    missing_dirs -= allowed_absent_gitlinks
    if missing_files or missing_dirs or (set(gitlinks) - seen_gitlinks) - allowed_absent_gitlinks:
        raise SeedError("raw source worktree does not match the frozen tracked-entry manifest")
    return total_bytes


def verify_source(
    source_root: Path,
    pin: SourcePin = EXACT_SOURCE,
    *,
    source_manifest_bytes: bytes | None = None,
) -> dict[str, Any]:
    source_root = Path(source_root)
    if not source_root.is_absolute() or (pin == EXACT_SOURCE and source_root != SOURCE_ROOT):
        raise SeedError("source root does not match the fixed source admission path")
    if source_manifest_bytes is None:
        if pin != EXACT_SOURCE:
            raise SeedError("custom source pins require an explicit test/source manifest")
        source_manifest_bytes = _read_trusted_absolute(
            SOURCE_MANIFEST_PATH, MAX_SOURCE_MANIFEST_BYTES
        )
    manifest = _parse_source_manifest(source_manifest_bytes, pin)
    source_fd = _open_trusted_directory(source_root)
    try:
        source_size = _check_source_tree(source_fd, manifest, source_root)
        lock_raw = _read_open_file_for_relative(
            source_fd, PurePosixPath("Cargo.lock"), source_root, MAX_LOCK_BYTES
        )
        if _sha256_bytes(lock_raw) != pin.lock_sha256:
            raise SeedError("Cargo.lock SHA-256 mismatch")
        try:
            lock = tomllib.loads(lock_raw.decode("utf-8"))
        except (UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
            raise SeedError("Cargo.lock is not valid UTF-8 TOML") from exc
        if lock.get("version") != pin.lock_version:
            raise SeedError(f"Cargo.lock version must be {pin.lock_version}")
        packages = lock.get("package")
        if not isinstance(packages, list) or len(packages) != pin.package_count:
            raise SeedError("Cargo.lock package list/count does not match the pin")
        root_manifest_raw = _read_open_file_for_relative(
            source_fd, PurePosixPath("Cargo.toml"), source_root, MAX_SOURCE_MANIFEST_BYTES
        )
        try:
            root_manifest = tomllib.loads(root_manifest_raw.decode("utf-8"))
        except (UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
            raise SeedError("root Cargo.toml is not valid UTF-8 TOML") from exc
        workspace = root_manifest.get("workspace")
        if not isinstance(workspace, dict) or not isinstance(workspace.get("members"), list):
            raise SeedError("root Cargo.toml must declare a workspace member list")
        workspace_package = workspace.get("package", {})

        local_by_name: dict[str, dict[str, str]] = {}
        seen_member_paths: set[str] = set()
        for member in workspace["members"]:
            if not isinstance(member, str) or any(token in member for token in ("*", "?", "[", "]")):
                raise SeedError("workspace member globs are not accepted by the bounded seed contract")
            relative = _safe_relative(member)
            if relative.as_posix() in seen_member_paths:
                raise SeedError(f"duplicate workspace member path: {member}")
            seen_member_paths.add(relative.as_posix())
            manifest_rel = relative / "Cargo.toml"
            try:
                member_manifest = tomllib.loads(
                    _read_open_file_for_relative(
                        source_fd, manifest_rel, source_root, MAX_SOURCE_MANIFEST_BYTES
                    ).decode("utf-8")
                )
            except (UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
                raise SeedError(f"invalid workspace member manifest: {member}") from exc
            package = member_manifest.get("package")
            if not isinstance(package, dict) or not isinstance(package.get("name"), str):
                raise SeedError(f"workspace member is not a package: {member}")
            name = package["name"]
            version = package.get("version")
            if isinstance(version, dict) and version.get("workspace") is True:
                version = workspace_package.get("version")
            if not isinstance(version, str) or name in local_by_name:
                raise SeedError(f"cannot resolve unique workspace package for {name}")
            local_by_name[name] = {"name": name, "version": version, "member": relative.as_posix()}

        registry_packages: list[dict[str, str]] = []
        lock_path_packages: dict[str, str] = {}
        for package in packages:
            name = package.get("name")
            version = package.get("version")
            if not isinstance(name, str) or not isinstance(version, str):
                raise SeedError("Cargo.lock package is missing a string name or version")
            source = package.get("source")
            if source is None:
                if name in lock_path_packages:
                    raise SeedError(f"duplicate local package in Cargo.lock: {name}")
                lock_path_packages[name] = version
                continue
            if source != REGISTRY_SOURCE:
                raise SeedError(f"unsupported non-crates.io lock source for {name} {version}")
            checksum = package.get("checksum")
            if not isinstance(checksum, str) or not SHA256.fullmatch(checksum):
                raise SeedError(f"invalid checksum in Cargo.lock for {name} {version}")
            if not CRATE_NAME.fullmatch(name) or not CRATE_VERSION.fullmatch(version):
                raise SeedError(f"unsafe crates.io package identity: {name} {version}")
            registry_packages.append({"name": name, "version": version, "checksum": checksum})

        if len(registry_packages) != pin.registry_package_count:
            raise SeedError(f"Cargo.lock crates.io package count must be {pin.registry_package_count}")
        if len(lock_path_packages) != pin.path_package_count:
            raise SeedError(f"Cargo.lock path package count must be {pin.path_package_count}")
        if len({item["name"] for item in registry_packages}) != pin.registry_name_count:
            raise SeedError(f"Cargo.lock crates.io name count must be {pin.registry_name_count}")
        if lock_path_packages != {name: item["version"] for name, item in local_by_name.items()}:
            raise SeedError("workspace member manifests do not match Cargo.lock path packages")

        if (
            _check_source_tree(source_fd, manifest, source_root) != source_size
            or _read_open_file_for_relative(
                source_fd, PurePosixPath("Cargo.lock"), source_root, MAX_LOCK_BYTES
            )
            != lock_raw
        ):
            raise SeedError("raw source changed while its manifest was being verified")

        registry_packages.sort(key=lambda item: (item["name"], item["version"]))
        path_packages = sorted(local_by_name.values(), key=lambda item: item["name"])
        return {
            "source_root": source_root,
            "commit": pin.commit,
            "tree": pin.tree,
            "source_manifest_sha256": pin.source_manifest_sha256,
            "source_size": source_size,
            "lock": lock,
            "lock_bytes": lock_raw,
            "lock_sha256": _sha256_bytes(lock_raw),
            "registry_packages": registry_packages,
            "path_packages": path_packages,
        }
    finally:
        os.close(source_fd)


@dataclass
class RegistryHandles:
    index_cache_fd: int
    cache_fd: int
    owned_fds: list[int]

    def close(self) -> None:
        for descriptor in reversed(self.owned_fds):
            try:
                os.close(descriptor)
            except OSError:
                pass
        self.owned_fds.clear()

    def __enter__(self) -> "RegistryHandles":
        return self

    def __exit__(self, *_exc) -> None:
        self.close()


def open_crates_io_registry(registry_root: Path) -> RegistryHandles:
    """Pin registry ancestry with root-owned, nofollow directory descriptors."""
    opened: list[int] = []
    try:
        root_fd = _open_trusted_directory(Path(registry_root))
        opened.append(root_fd)
        index_base = _open_directory_at(root_fd, "index", str(registry_root / "index"))
        opened.append(index_base)
        cache_base = _open_directory_at(root_fd, "cache", str(registry_root / "cache"))
        opened.append(cache_base)
        index_names = sorted(name for name in os.listdir(index_base) if name.startswith("index.crates.io-"))
        cache_names = sorted(name for name in os.listdir(cache_base) if name.startswith("index.crates.io-"))
        if len(index_names) != 1 or cache_names != index_names:
            raise SeedError("expected exactly one matching crates.io index and archive cache")
        index_root = _open_directory_at(index_base, index_names[0], "crates.io index root")
        opened.append(index_root)
        cache_root = _open_directory_at(cache_base, cache_names[0], "crates.io archive root")
        opened.append(cache_root)
        index_cache = _open_directory_at(index_root, ".cache", "crates.io sparse cache")
        opened.append(index_cache)
        return RegistryHandles(index_cache_fd=index_cache, cache_fd=cache_root, owned_fds=opened)
    except BaseException:
        for descriptor in reversed(opened):
            try:
                os.close(descriptor)
            except OSError:
                pass
        raise


def _read_registry_relative(root_fd: int, relative: PurePosixPath, max_bytes: int, label: str) -> bytes:
    stream, _info = _open_relative_file(root_fd, relative, label)
    with stream:
        return _read_open_file(stream, Path(label), max_bytes)


def _index_record(index_cache_fd: int, package: dict[str, str]) -> bytes:
    index_file = index_relative_path(package["name"])
    raw = _read_registry_relative(
        index_cache_fd, index_file, MAX_INDEX_CACHE_ENTRY_BYTES, f"sparse index/{index_file}"
    )
    match: bytes | None = None
    record_count = 0
    offset = 0
    while offset < len(raw):
        end = raw.find(b"\0", offset)
        if end < 0:
            end = len(raw)
        part = raw[offset:end]
        offset = end + 1
        if not part:
            continue
        record_count += 1
        if record_count > MAX_INDEX_CACHE_RECORDS:
            raise SeedError(f"registry index cache has too many rows: {package['name']}")
        if len(part) > MAX_INDEX_RECORD_BYTES:
            raise SeedError(f"registry index record exceeds size limit: {package['name']}")
        try:
            record = json.loads(part)
        except (json.JSONDecodeError, UnicodeDecodeError, RecursionError):
            continue
        if (
            isinstance(record, dict)
            and record.get("name") == package["name"]
            and record.get("vers") == package["version"]
        ):
            if record.get("cksum") != package["checksum"]:
                raise SeedError(
                    f"index checksum disagrees with Cargo.lock for {package['name']} {package['version']}"
                )
            if match is not None:
                raise SeedError(
                    f"duplicate matching index record for {package['name']} {package['version']}"
                )
            match = part
    if match is None:
        raise SeedError(
            f"expected exactly one matching index record for {package['name']} {package['version']}"
        )
    return match


def _safe_archive_member(member: tarfile.TarInfo, crate_root: str) -> PurePosixPath:
    relative = _safe_relative(member.name.rstrip("/"))
    if not relative.parts or relative.parts[0] != crate_root:
        raise SeedError(f"crate archive member is outside {crate_root}: {member.name!r}")
    if not (member.isdir() or member.isreg()):
        raise SeedError(f"unsupported non-file archive member: {member.name!r}")
    return PurePosixPath(*relative.parts[1:])


class _LimitedReader:
    def __init__(self, source: BinaryIO, limit: int, label: Path):
        self.source = source
        self.limit = limit
        self.label = label
        self.total = 0

    def read(self, size: int = -1) -> bytes:
        request = self.limit - self.total + 1
        if size >= 0:
            request = min(request, size)
        chunk = self.source.read(request)
        self.total += len(chunk)
        if self.total > self.limit:
            raise SeedError(f"decompressed archive exceeds size limit: {self.label}")
        return chunk

    def tell(self) -> int:
        return self.total

    def readable(self) -> bool:
        return True


@contextmanager
def _open_archive_members(archive: Path):
    compressed = _read_regular_file(archive, max_bytes=MAX_ARCHIVE_BYTES)
    try:
        with gzip.GzipFile(fileobj=io.BytesIO(compressed), mode="rb") as inflated:
            limited = _LimitedReader(inflated, MAX_PACKAGE_TAR_BYTES, archive)
            with tarfile.open(fileobj=limited, mode="r|") as tar:
                yield tar
    except (OSError, EOFError, tarfile.TarError) as exc:
        raise SeedError(f"invalid crate archive: {archive.name}") from exc


def _extract_archive(
    archive: Path,
    destination: Path,
    package: dict[str, str],
    total_counter: list[int],
) -> list[dict[str, Any]]:
    expected_root = f"{package['name']}-{package['version']}"
    destination.mkdir(parents=True, exist_ok=False)
    files: list[dict[str, Any]] = []
    seen: set[str] = set()
    package_bytes = 0
    try:
        with _open_archive_members(archive) as tar:
            member_count = 0
            for member in tar:
                member_count += 1
                if member_count > MAX_PACKAGE_FILES:
                    raise SeedError(f"too many files in crate archive: {expected_root}")
                relative = _safe_archive_member(member, expected_root)
                if not relative.parts:
                    if not member.isdir():
                        raise SeedError(f"crate root is not a directory: {expected_root}")
                    continue
                relative_text = relative.as_posix()
                if relative_text in seen:
                    raise SeedError(f"duplicate crate archive member: {expected_root}/{relative_text}")
                seen.add(relative_text)
                target = destination.joinpath(*relative.parts)
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                    target.chmod(0o755)
                    continue
                if member.size < 0:
                    raise SeedError(f"invalid archive member size: {member.name}")
                package_bytes += member.size
                total_counter[0] += member.size
                if package_bytes > MAX_PACKAGE_UNPACKED_BYTES:
                    raise SeedError(f"crate exceeds unpacked size limit: {expected_root}")
                if total_counter[0] > MAX_TOTAL_UNPACKED_BYTES:
                    raise SeedError("seed exceeds total unpacked size limit")
                target.parent.mkdir(parents=True, exist_ok=True)
                mode = 0o755 if member.mode & 0o111 else 0o644
                source = tar.extractfile(member)
                if source is None:
                    raise SeedError(f"cannot read crate archive member: {member.name}")
                digest = hashlib.sha256()
                written = 0
                with source, target.open("xb") as output:
                    while chunk := source.read(1024 * 1024):
                        if written + len(chunk) > member.size:
                            raise SeedError(f"crate archive member exceeded declared size: {member.name}")
                        if package_bytes - member.size + written + len(chunk) > MAX_PACKAGE_UNPACKED_BYTES:
                            raise SeedError(f"crate exceeds unpacked size limit: {expected_root}")
                        if total_counter[0] - member.size + written + len(chunk) > MAX_TOTAL_UNPACKED_BYTES:
                            raise SeedError("seed exceeds total unpacked size limit")
                        written += len(chunk)
                        digest.update(chunk)
                        output.write(chunk)
                if written != member.size:
                    raise SeedError(f"crate archive member size mismatch: {member.name}")
                target.chmod(mode)
                files.append(
                    {
                        "path": relative_text,
                        "size": written,
                        "mode": mode,
                        "sha256": digest.hexdigest(),
                    }
                )
    except (OSError, tarfile.TarError) as exc:
        raise SeedError(f"invalid crate archive for {expected_root}") from exc

    files.sort(key=lambda item: item["path"])
    manifest_path = destination / "Cargo.toml"
    if not manifest_path.is_file() or manifest_path.is_symlink():
        raise SeedError(f"crate archive has no regular Cargo.toml: {expected_root}")
    try:
        source_manifest = tomllib.loads(
            _read_regular_file(manifest_path, max_bytes=MAX_SOURCE_MANIFEST_BYTES).decode("utf-8")
        )
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise SeedError(f"invalid extracted Cargo.toml for {expected_root}") from exc
    package_data = source_manifest.get("package")
    if not isinstance(package_data, dict):
        raise SeedError(f"extracted Cargo.toml has no [package]: {expected_root}")
    if package_data.get("name") != package["name"] or package_data.get("version") != package["version"]:
        raise SeedError(f"extracted Cargo.toml identity mismatch: {expected_root}")
    return files


def _write_json(path: Path, value: Any, *, max_bytes: int) -> tuple[str, int]:
    encoder = json.JSONEncoder(sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    digest = hashlib.sha256()
    written = 0
    output, identity = _open_new_regular_binary(path)
    try:
        with output:
            for piece in encoder.iterencode(value):
                chunk = piece.encode("utf-8")
                written += len(chunk)
                if written > max_bytes:
                    raise SeedError("candidate manifest exceeds size limit")
                output.write(chunk)
                digest.update(chunk)
            chunk = b"\n"
            written += 1
            if written > max_bytes:
                raise SeedError("candidate manifest exceeds size limit")
            output.write(chunk)
            digest.update(chunk)
        path.chmod(0o644)
    except BaseException:
        _unlink_created_regular(path, identity)
        raise
    return digest.hexdigest(), written


def _copy_verified_archive(
    cache_fd: int, archive_name: str, destination: Path, package: dict[str, str]
) -> str:
    destination.parent.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha256()
    destination_identity: tuple[int, int] | None = None
    try:
        src, _info = _open_file_at(cache_fd, archive_name, f"crate cache/{archive_name}")
        with src:
            dst, destination_identity = _open_new_regular_binary(destination)
            with dst:
                def write_chunk(chunk: bytes) -> None:
                    dst.write(chunk)
                    digest.update(chunk)

                _consume_bounded(src, Path(archive_name), MAX_ARCHIVE_BYTES, write_chunk)
        checksum = digest.hexdigest()
        if checksum != package["checksum"]:
            raise SeedError(f"crate archive checksum mismatch: {package['name']} {package['version']}")
        destination.chmod(0o644)
        return checksum
    except FileNotFoundError as exc:
        _unlink_created_regular(destination, destination_identity)
        raise SeedError(f"missing regular crate archive for {package['name']} {package['version']}") from exc
    except BaseException:
        _unlink_created_regular(destination, destination_identity)
        raise


def prepare_candidate(
    source_root: Path,
    registry_root: Path,
    output: Path,
    *,
    pin: SourcePin = EXACT_SOURCE,
    private_root: Path = PRIVATE_ROOT,
    source_manifest_bytes: bytes | None = None,
) -> Path:
    """Create a new capsule from one explicit registry root, with no network."""
    private_root = Path(private_root)
    output = Path(output)
    output_parts = _relative_parts_under(output, private_root)
    source_data = verify_source(source_root, pin, source_manifest_bytes=source_manifest_bytes)
    registry = open_crates_io_registry(registry_root)
    private_fd = -1
    parent_fd = -1
    output_fd = -1
    created = False
    try:
        private_fd = _open_trusted_directory(private_root)
        parent_fd = _open_directory_chain(private_fd, output_parts[:-1], create=True)
        leaf = output_parts[-1]
        os.mkdir(leaf, 0o755, dir_fd=parent_fd)
        created = True
        output_fd = _open_directory_at(parent_fd, leaf, str(output))
        with registry:
            return _prepare_candidate_in_output(
                source_data, registry, output, output_fd, package_parent_fd=parent_fd,
                package_leaf=leaf, pin=pin,
            )
    except BaseException:
        if created and parent_fd >= 0:
            _remove_tree_at(parent_fd, output_parts[-1])
        raise
    finally:
        if output_fd >= 0:
            os.close(output_fd)
        if parent_fd >= 0:
            os.close(parent_fd)
        if private_fd >= 0:
            os.close(private_fd)
        registry.close()


def _prepare_candidate_in_output(
    source_data: dict[str, Any],
    registry: RegistryHandles,
    output: Path,
    output_fd: int,
    *,
    package_parent_fd: int,
    package_leaf: str,
    pin: SourcePin,
) -> Path:
    _check_trusted_directory(output_fd, str(output))
    total_counter = [0]
    package_records: list[dict[str, Any]] = []
    try:
        for package in source_data["registry_packages"]:
            name = package["name"]
            version = package["version"]
            package_key = f"{name}-{version}"
            index_record = _index_record(registry.index_cache_fd, package)
            index_path = index_relative_path(name) / f"{version}.json"
            index_destination = output / "index-records" / index_path
            index_destination.parent.mkdir(parents=True, exist_ok=True)
            with index_destination.open("xb") as file:
                file.write(index_record)
                file.write(b"\n")
            index_destination.chmod(0o644)

            archive_destination = output / "crates" / f"{package_key}.crate"
            archive_hash = _copy_verified_archive(
                registry.cache_fd, f"{package_key}.crate", archive_destination, package
            )
            source_destination = output / "sources" / package_key
            source_files = _extract_archive(archive_destination, source_destination, package, total_counter)
            source_manifest_hash = _sha256_file(source_destination / "Cargo.toml")
            package_records.append(
                {
                    **package,
                    "index_path": index_path.as_posix(),
                    "index_record_sha256": _sha256_bytes(index_record),
                    "archive_sha256": archive_hash,
                    "source_manifest_path": f"sources/{package_key}/Cargo.toml",
                    "source_manifest_sha256": source_manifest_hash,
                    "source_files": source_files,
                }
            )

        manifest = {
            "format": FORMAT,
            "source": {
                "commit": source_data["commit"],
                "tree": source_data["tree"],
                "lock_sha256": source_data["lock_sha256"],
                "source_manifest_sha256": source_data["source_manifest_sha256"],
                "lock_version": pin.lock_version,
            },
            "registry": REGISTRY_SOURCE,
            "counts": {
                "lock_packages": pin.package_count,
                "registry_packages": len(package_records),
                "registry_names": len({item['name'] for item in package_records}),
                "path_packages": len(source_data["path_packages"]),
                "source_files": sum(len(item["source_files"]) for item in package_records),
                "source_bytes": total_counter[0],
            },
            "path_packages": source_data["path_packages"],
            "packages": package_records,
        }
        manifest_hash, _manifest_size = _write_json(
            output / "manifest.json", manifest, max_bytes=MAX_CANDIDATE_MANIFEST_BYTES
        )
        (output / "manifest.sha256").write_text(
            manifest_hash + "  manifest.json\n", encoding="ascii"
        )
        (output / "manifest.sha256").chmod(0o644)
        _normalize_candidate_metadata(output)
        return output
    except BaseException:
        _remove_tree_at(package_parent_fd, package_leaf)
        raise


def build_plan(
    source_root: Path,
    pin: SourcePin = EXACT_SOURCE,
    *,
    source_manifest_bytes: bytes | None = None,
) -> dict[str, Any]:
    """Return the source/lock-derived bounded acquisition plan."""
    source_data = verify_source(source_root, pin, source_manifest_bytes=source_manifest_bytes)
    registry = []
    for package in source_data["registry_packages"]:
        name = package["name"]
        version = package["version"]
        key = f"{name}-{version}"
        registry.append(
            {
                **package,
                "index_record_path": (index_relative_path(name) / f"{version}.json").as_posix(),
                "archive_path": f"crates/{key}.crate",
                "source_directory": f"sources/{key}",
                "source_manifest_path": f"sources/{key}/Cargo.toml",
            }
        )
    return {
        "format": "velnor-cargo-seed-plan-v3",
        "source": {
            "commit": source_data["commit"],
            "tree": source_data["tree"],
            "lock_sha256": source_data["lock_sha256"],
            "source_manifest_sha256": source_data["source_manifest_sha256"],
            "lock_version": pin.lock_version,
        },
        "registry": REGISTRY_SOURCE,
        "scope": {
            "targets": "all lockfile targets; no platform filter",
            "dependency_kinds": "all lockfile entries, including build and dev dependencies",
            "git_sources": 0,
            "index_rows": "one exact name/version/checksum row per registry package instance",
            "source_content": "extracted only from checksum-verified .crate archives",
            "execution": "metadata only; no package build scripts or code are run by these scripts",
        },
        "counts": {
            "lock_packages": pin.package_count,
            "registry_packages": len(registry),
            "registry_names": len({item["name"] for item in registry}),
            "path_packages": len(source_data["path_packages"]),
        },
        "path_packages": source_data["path_packages"],
        "registry_packages": registry,
    }


def write_plan(
    source_root: Path,
    output: Path,
    *,
    pin: SourcePin = EXACT_SOURCE,
    source_manifest_bytes: bytes | None = None,
) -> Path:
    private_root = PRIVATE_ROOT
    output_parts = _relative_parts_under(Path(output), private_root)
    if len(output_parts) != 1:
        raise SeedError("plan output must be one new file directly under the assigned task root")
    plan = build_plan(source_root, pin, source_manifest_bytes=source_manifest_bytes)
    private_fd = _open_trusted_directory(private_root)
    identity: tuple[int, int] | None = None
    try:
        output_stream, identity = _open_new_regular_at(private_fd, output_parts[0])
        digest = hashlib.sha256()
        written = 0
        encoder = json.JSONEncoder(sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        try:
            with output_stream:
                for piece in encoder.iterencode(plan):
                    chunk = piece.encode("utf-8")
                    written += len(chunk)
                    if written > MAX_PLAN_BYTES:
                        raise SeedError("source plan exceeds size limit")
                    output_stream.write(chunk)
                    digest.update(chunk)
                output_stream.write(b"\n")
                written += 1
                if written > MAX_PLAN_BYTES:
                    raise SeedError("source plan exceeds size limit")
                os.fchmod(output_stream.fileno(), 0o644)
            os.utime(output_parts[0], (0, 0), dir_fd=private_fd, follow_symlinks=False)
        except BaseException:
            _unlink_created_at(private_fd, output_parts[0], identity)
            raise
    finally:
        os.close(private_fd)
    return private_root / output_parts[0]


def _candidate_file_map(candidate: Path) -> dict[str, Path]:
    found: dict[str, Path] = {}
    for current, dirnames, filenames in os.walk(candidate, followlinks=False):
        current_path = Path(current)
        for dirname in dirnames:
            path = current_path / dirname
            if path.is_symlink():
                raise SeedError(f"candidate contains a symlink: {path.relative_to(candidate)}")
        for filename in filenames:
            path = current_path / filename
            if path.is_symlink() or not path.is_file():
                raise SeedError(f"candidate contains a non-regular file: {path.relative_to(candidate)}")
            relative = path.relative_to(candidate).as_posix()
            found[relative] = path
    return found


def verify_candidate(
    candidate: Path,
    *,
    source_root: Path = SOURCE_ROOT,
    pin: SourcePin = EXACT_SOURCE,
    private_root: Path = PRIVATE_ROOT,
    source_manifest_bytes: bytes | None = None,
) -> dict[str, Any]:
    private_root = Path(private_root)
    root_fd = _open_trusted_directory(private_root)
    candidate_fd = -1
    try:
        parts = _relative_parts_under(Path(candidate), private_root)
        candidate_fd = _open_directory_chain(root_fd, parts, create=False)
        _check_trusted_directory(candidate_fd, str(candidate))
        candidate_path = private_root.joinpath(*parts)
        return _verify_candidate_contents(
            candidate_path,
            source_root=source_root,
            pin=pin,
            private_root=private_root,
            source_manifest_bytes=source_manifest_bytes,
        )
    finally:
        if candidate_fd >= 0:
            os.close(candidate_fd)
        os.close(root_fd)


def _verify_candidate_contents(
    candidate: Path,
    *,
    source_root: Path,
    pin: SourcePin = EXACT_SOURCE,
    private_root: Path = PRIVATE_ROOT,
    source_manifest_bytes: bytes | None = None,
) -> dict[str, Any]:
    """Verify capsule contents offline; no network or package code is run."""
    source_data = verify_source(source_root, pin, source_manifest_bytes=source_manifest_bytes)

    files = _candidate_file_map(candidate)
    manifest_path = candidate / "manifest.json"
    manifest_hash_path = candidate / "manifest.sha256"
    if "manifest.json" not in files or "manifest.sha256" not in files:
        raise SeedError("candidate lacks manifest.json or manifest.sha256")
    manifest_bytes = _read_regular_file(manifest_path, max_bytes=MAX_CANDIDATE_MANIFEST_BYTES)
    manifest_hash = _sha256_bytes(manifest_bytes)
    expected_hash_line = f"{manifest_hash}  manifest.json\n"
    try:
        actual_hash_line = _read_regular_file(
            manifest_hash_path, max_bytes=MAX_MANIFEST_HASH_BYTES
        ).decode("ascii")
    except UnicodeDecodeError as exc:
        raise SeedError("manifest.sha256 is not ASCII") from exc
    if actual_hash_line != expected_hash_line:
        raise SeedError("manifest.sha256 does not match manifest.json")
    try:
        manifest = json.loads(manifest_bytes)
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        raise SeedError("candidate manifest is invalid JSON") from exc
    if not isinstance(manifest, dict) or manifest.get("format") != FORMAT:
        raise SeedError("unsupported candidate manifest format")
    if set(manifest) != {"format", "source", "registry", "counts", "path_packages", "packages"}:
        raise SeedError("candidate manifest contains unexpected fields")
    source = manifest.get("source")
    if not isinstance(source, dict):
        raise SeedError("candidate manifest is missing source identity")
    if source != {
        "commit": pin.commit,
        "tree": pin.tree,
        "lock_sha256": pin.lock_sha256,
        "source_manifest_sha256": pin.source_manifest_sha256,
        "lock_version": pin.lock_version,
    }:
        raise SeedError("candidate source identity does not match the pin")
    if manifest.get("registry") != REGISTRY_SOURCE:
        raise SeedError("candidate registry source mismatch")
    packages = manifest.get("packages")
    path_packages = manifest.get("path_packages")
    if not isinstance(packages, list) or not isinstance(path_packages, list):
        raise SeedError("candidate package lists are invalid")
    if any(not isinstance(package, dict) for package in packages):
        raise SeedError("candidate package list contains a non-object")
    package_names = [package.get("name") for package in packages]
    if any(not isinstance(name, str) for name in package_names):
        raise SeedError("candidate package list contains an invalid name")
    if len(packages) != pin.registry_package_count or len(path_packages) != pin.path_package_count:
        raise SeedError("candidate package counts do not match the pin")
    if len(set(package_names)) != pin.registry_name_count:
        raise SeedError("candidate registry name count does not match the pin")
    expected_packages = source_data["registry_packages"]
    if [{k: p.get(k) for k in ("name", "version", "checksum")} for p in packages] != expected_packages:
        raise SeedError("candidate registry packages do not match Cargo.lock")
    if path_packages != source_data["path_packages"]:
        raise SeedError("candidate path packages do not match the source workspace")

    expected_paths = {"manifest.json", "manifest.sha256"}
    seen_packages: set[tuple[str, str]] = set()
    total_source_bytes = 0
    total_source_files = 0
    for package in packages:
        if not isinstance(package, dict):
            raise SeedError("candidate package entry is not an object")
        if set(package) != {
            "name",
            "version",
            "checksum",
            "index_path",
            "index_record_sha256",
            "archive_sha256",
            "source_manifest_path",
            "source_manifest_sha256",
            "source_files",
        }:
            raise SeedError("candidate package contains unexpected fields")
        name = package.get("name")
        version = package.get("version")
        checksum = package.get("checksum")
        if not isinstance(name, str) or not CRATE_NAME.fullmatch(name):
            raise SeedError("invalid candidate package name")
        if not isinstance(version, str) or not CRATE_VERSION.fullmatch(version):
            raise SeedError("invalid candidate package version")
        if not isinstance(checksum, str) or not SHA256.fullmatch(checksum):
            raise SeedError("invalid candidate package checksum")
        identity = (name, version)
        if identity in seen_packages:
            raise SeedError(f"duplicate candidate package: {name} {version}")
        seen_packages.add(identity)
        key = f"{name}-{version}"
        archive_rel = f"crates/{key}.crate"
        source_rel = f"sources/{key}"
        expected_index_path = (index_relative_path(name) / f"{version}.json").as_posix()
        index_rel = f"index-records/{expected_index_path}"
        if package.get("index_path") != expected_index_path:
            raise SeedError(f"wrong index record path for {name} {version}")
        for relative in (archive_rel, index_rel):
            expected_paths.add(relative)
            if relative not in files:
                raise SeedError(f"candidate is missing {relative}")
        archive_path = candidate / archive_rel
        if archive_path.stat().st_size > MAX_ARCHIVE_BYTES:
            raise SeedError(f"candidate archive exceeds size limit for {name} {version}")
        if (
            _sha256_file(archive_path, max_bytes=MAX_ARCHIVE_BYTES) != checksum
            or package.get("archive_sha256") != checksum
        ):
            raise SeedError(f"archive checksum mismatch for {name} {version}")
        index_bytes = _read_regular_file(
            candidate / index_rel, max_bytes=MAX_INDEX_RECORD_BYTES + 1
        )
        if not index_bytes.endswith(b"\n"):
            raise SeedError(f"index record lacks final newline for {name} {version}")
        record_bytes = index_bytes[:-1]
        try:
            record = json.loads(record_bytes)
        except (json.JSONDecodeError, UnicodeDecodeError) as exc:
            raise SeedError(f"invalid index record for {name} {version}") from exc
        if not isinstance(record, dict):
            raise SeedError(f"invalid index record object for {name} {version}")
        if (
            record.get("name") != name
            or record.get("vers") != version
            or record.get("cksum") != checksum
            or package.get("index_record_sha256") != _sha256_bytes(record_bytes)
        ):
            raise SeedError(f"index record does not match Cargo.lock for {name} {version}")
        source_manifest_rel = package.get("source_manifest_path")
        expected_source_manifest_rel = f"{source_rel}/Cargo.toml"
        if source_manifest_rel != expected_source_manifest_rel:
            raise SeedError(f"wrong source manifest path for {name} {version}")
        manifest_file_records = package.get("source_files")
        if not isinstance(manifest_file_records, list):
            raise SeedError(f"source file inventory missing for {name} {version}")
        source_dir = candidate / source_rel
        actual_source_files: dict[str, Path] = {}
        for current, dirnames, filenames in os.walk(source_dir, followlinks=False):
            current_path = Path(current)
            for dirname in dirnames:
                if (current_path / dirname).is_symlink():
                    raise SeedError(f"source tree contains a symlink: {name} {version}")
            for filename in filenames:
                file_path = current_path / filename
                if file_path.is_symlink() or not file_path.is_file():
                    raise SeedError(f"source tree contains a non-regular file: {name} {version}")
                actual_source_files[file_path.relative_to(source_dir).as_posix()] = file_path
        record_paths: set[str] = set()
        for item in manifest_file_records:
            if not isinstance(item, dict):
                raise SeedError(f"invalid source inventory entry: {name} {version}")
            relative_value = item.get("path", "")
            if not isinstance(relative_value, str):
                raise SeedError(f"invalid source inventory path: {name} {version}")
            relative = _safe_relative(relative_value).as_posix()
            if relative in record_paths:
                raise SeedError(f"duplicate source inventory path: {name} {version}")
            record_paths.add(relative)
            source_file = actual_source_files.get(relative)
            if source_file is None:
                raise SeedError(f"candidate source file is missing: {name} {version}/{relative}")
            item_size = item.get("size")
            item_mode = item.get("mode")
            item_hash = item.get("sha256")
            if type(item_size) is not int or item_size < 0:
                raise SeedError(f"invalid source file size: {name} {version}/{relative}")
            if item_mode not in (0o644, 0o755) or not isinstance(item_hash, str) or not SHA256.fullmatch(item_hash):
                raise SeedError(f"invalid source file metadata: {name} {version}/{relative}")
            if (
                _sha256_file(source_file) != item_hash
                or source_file.stat().st_size != item_size
                or stat.S_IMODE(source_file.stat().st_mode) != item_mode
            ):
                raise SeedError(f"candidate source file mismatch: {name} {version}/{relative}")
            expected_paths.add(f"{source_rel}/{relative}")
            total_source_bytes += item_size
            if total_source_bytes > MAX_TOTAL_UNPACKED_BYTES:
                raise SeedError("candidate source inventory exceeds total size limit")
            total_source_files += 1
        if record_paths != set(actual_source_files):
            raise SeedError(f"source inventory does not cover all files: {name} {version}")
        source_manifest_path = candidate / expected_source_manifest_rel
        if _sha256_file(source_manifest_path) != package.get("source_manifest_sha256"):
            raise SeedError(f"source manifest checksum mismatch: {name} {version}")
        try:
            source_manifest = tomllib.loads(
                _read_regular_file(
                    source_manifest_path, max_bytes=MAX_SOURCE_MANIFEST_BYTES
                ).decode("utf-8")
            )
        except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
            raise SeedError(f"invalid source manifest: {name} {version}") from exc
        source_package = source_manifest.get("package", {})
        if not isinstance(source_package, dict):
            raise SeedError(f"source manifest package section is invalid: {name} {version}")
        if source_package.get("name") != name or source_package.get("version") != version:
            raise SeedError(f"source manifest identity mismatch: {name} {version}")
        _verify_archive_matches_sources(archive_path, source_dir, package)

    actual_paths = set(files)
    if actual_paths != expected_paths:
        extras = sorted(actual_paths - expected_paths)
        missing = sorted(expected_paths - actual_paths)
        raise SeedError(f"candidate file set mismatch (extra={extras[:3]}, missing={missing[:3]})")
    counts = manifest.get("counts")
    if not isinstance(counts, dict):
        raise SeedError("candidate counts are missing")
    expected_counts = {
        "lock_packages": pin.package_count,
        "registry_packages": pin.registry_package_count,
        "registry_names": pin.registry_name_count,
        "path_packages": pin.path_package_count,
        "source_files": total_source_files,
        "source_bytes": total_source_bytes,
    }
    if counts != expected_counts:
        raise SeedError("candidate counts do not match its verified contents")
    return {"manifest_sha256": manifest_hash, "counts": counts}


def main_prepare(argv: Iterable[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry-root", required=True, type=Path, help="explicit Cargo registry root; config is never read")
    args = parser.parse_args(argv)
    try:
        verify_host_identity()
        result = prepare_candidate(SOURCE_ROOT, args.registry_root, CANDIDATE_OUTPUT)
    except SeedError as exc:
        parser.error(str(exc))
    print(result)
    return 0


def main_plan(argv: Iterable[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Write the exact source/lock-derived seed plan.")
    parser.parse_args(argv)
    try:
        verify_host_identity()
        result = write_plan(SOURCE_ROOT, PLAN_OUTPUT)
    except SeedError as exc:
        parser.error(str(exc))
    print(result)
    return 0


def main_verify(argv: Iterable[str] | None = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Verify a Cargo seed capsule offline.")
    parser.parse_args(argv)
    try:
        verify_host_identity()
        result = verify_candidate(CANDIDATE_OUTPUT, source_root=SOURCE_ROOT)
    except SeedError as exc:
        parser.error(str(exc))
    print(json.dumps(result, sort_keys=True))
    return 0


def _verify_archive_matches_sources(archive: Path, source_root: Path, package: dict[str, str]) -> None:
    crate_root = f"{package['name']}-{package['version']}"
    if archive.stat().st_size > MAX_ARCHIVE_BYTES:
        raise SeedError(f"archive exceeds compressed size limit for {crate_root}")
    archive_files: dict[str, tuple[int, int, str]] = {}
    unpacked_total = 0
    member_count = 0
    try:
        with _open_archive_members(archive) as tar:
            for member in tar:
                member_count += 1
                if member_count > MAX_PACKAGE_FILES:
                    raise SeedError(f"too many archive members for {crate_root}")
                relative = _safe_archive_member(member, crate_root)
                if not relative.parts:
                    if not member.isdir():
                        raise SeedError(f"crate archive root is not a directory: {crate_root}")
                    continue
                if member.isdir():
                    continue
                if member.size < 0 or member.size > MAX_PACKAGE_UNPACKED_BYTES:
                    raise SeedError(f"archive member exceeds size limit for {crate_root}")
                if unpacked_total + member.size > MAX_PACKAGE_UNPACKED_BYTES:
                    raise SeedError(f"archive exceeds unpacked size limit for {crate_root}")
                source = tar.extractfile(member)
                if source is None:
                    raise SeedError(f"cannot read archive member for {crate_root}")
                digest = hashlib.sha256()
                size = 0
                with source:
                    while chunk := source.read(1024 * 1024):
                        digest.update(chunk)
                        size += len(chunk)
                if size != member.size:
                    raise SeedError(f"archive member size mismatch for {crate_root}")
                unpacked_total += size
                if unpacked_total > MAX_PACKAGE_UNPACKED_BYTES:
                    raise SeedError(f"archive exceeds unpacked size limit for {crate_root}")
                relative_text = relative.as_posix()
                if relative_text in archive_files:
                    raise SeedError(f"duplicate archive file for {crate_root}/{relative_text}")
                archive_files[relative_text] = (
                    size,
                    0o755 if member.mode & 0o111 else 0o644,
                    digest.hexdigest(),
                )
    except (OSError, tarfile.TarError) as exc:
        raise SeedError(f"cannot validate archive contents for {crate_root}") from exc
    source_files: dict[str, tuple[int, int, str]] = {}
    for current, dirnames, filenames in os.walk(source_root, followlinks=False):
        current_path = Path(current)
        if any((current_path / name).is_symlink() for name in dirnames):
            raise SeedError(f"candidate source tree contains a symlink: {crate_root}")
        for filename in filenames:
            path = current_path / filename
            if path.is_symlink() or not path.is_file():
                raise SeedError(f"candidate source tree contains a non-regular file: {crate_root}")
            relative = path.relative_to(source_root).as_posix()
            source_files[relative] = (
                path.stat().st_size,
                stat.S_IMODE(path.stat().st_mode),
                _sha256_file(path),
            )
    if archive_files != source_files:
        raise SeedError(f"extracted source does not match archive for {crate_root}")


def _normalize_candidate_metadata(root: Path) -> None:
    for current, dirnames, filenames in os.walk(root, topdown=False, followlinks=False):
        current_path = Path(current)
        for filename in filenames:
            path = current_path / filename
            if path.is_symlink():
                raise SeedError("candidate output unexpectedly contains a symlink")
            mode = stat.S_IMODE(path.stat().st_mode)
            path.chmod(0o755 if mode & 0o111 else 0o644)
            os.utime(path, (0, 0), follow_symlinks=False)
        for dirname in dirnames:
            path = current_path / dirname
            if path.is_symlink():
                raise SeedError("candidate output unexpectedly contains a symlink")
            path.chmod(0o755)
            os.utime(path, (0, 0), follow_symlinks=False)
        current_path.chmod(0o755)
        os.utime(current_path, (0, 0), follow_symlinks=False)


if __name__ == "__main__":
    import sys

    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "plan":
        raise SystemExit(main_plan(sys.argv[2:]))
    if command == "prepare":
        raise SystemExit(main_prepare(sys.argv[2:]))
    if command == "verify":
        raise SystemExit(main_verify(sys.argv[2:]))
    raise SystemExit("usage: seed_candidate_v3.py {plan|prepare|verify} ...")
