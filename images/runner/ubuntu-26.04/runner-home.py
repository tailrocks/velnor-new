#!/usr/bin/env python3
"""This program does a check of the Runner seed and copies its files into a fresh Runner home volume."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import posixpath
import secrets
import shutil
import stat
import sys

SCHEMA_VERSION = 1
HOME_LIMIT_BYTES = 2 * 1024 * 1024 * 1024
HOME_ALLOWANCE_BYTES = 512 * 1024 * 1024
MAX_ENTRIES = 50_000
MAX_MANIFEST_BYTES = 32 * 1024 * 1024
CHUNK_BYTES = 1024 * 1024
RUNNER_HOME = "/home/runner"
RUNNER_WORK = "/home/runner/_work"


class SeedError(Exception):
    """Runner seed or Runner home volume copy failed."""


def fail(reason: str) -> SeedError:
    return SeedError(reason)


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def decode_mount_path(value: str) -> str:
    replacements = {"040": " ", "011": "\t", "012": "\n", "134": "\\"}
    out = []
    index = 0
    while index < len(value):
        if value[index] == "\\" and index + 3 < len(value):
            code = value[index + 1 : index + 4]
            if code in replacements:
                out.append(replacements[code])
                index += 4
                continue
        out.append(value[index])
        index += 1
    return "".join(out)


def mount_targets(mountinfo: str) -> set[str]:
    targets = set()
    for line in mountinfo.splitlines():
        fields = line.split(" - ", 1)[0].split()
        if len(fields) < 5:
            raise fail("mountinfo_format")
        targets.add(decode_mount_path(fields[4]))
    return targets


def validate_symlink(relative: str, target: str) -> None:
    if not target or target.startswith("/") or "\x00" in target or len(target) > 4096:
        raise fail("seed_symlink_target")
    resolved = posixpath.normpath(posixpath.join(posixpath.dirname(relative), target))
    if resolved == ".." or resolved.startswith("../"):
        raise fail("seed_symlink_escape")


def allocated_bytes(info: os.stat_result, kind: str) -> int:
    if kind == "directory":
        return max(4096, info.st_blocks * 512)
    logical = info.st_size
    physical = info.st_blocks * 512
    rounded = ((max(logical, physical) + 4095) // 4096) * 4096
    return max(4096, rounded)


def file_digest(path: str, expected: os.stat_result | None = None) -> str:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    digest = hashlib.sha256()
    with os.fdopen(descriptor, "rb", closefd=True) as source:
        before = os.fstat(source.fileno())
        if not stat.S_ISREG(before.st_mode):
            raise fail("seed_file_type")
        if expected is not None and (before.st_dev, before.st_ino) != (expected.st_dev, expected.st_ino):
            raise fail("seed_file_changed")
        while True:
            chunk = source.read(CHUNK_BYTES)
            if not chunk:
                break
            digest.update(chunk)
        after = os.fstat(source.fileno())
        if (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        ):
            raise fail("seed_file_changed")
    return digest.hexdigest()


def inventory(
    root: str,
    *,
    exclude_work: bool = False,
    expected_owner: tuple[int, int] | None = None,
) -> tuple[list[dict[str, object]], int]:
    root_path = os.path.abspath(root)
    try:
        root_info = os.lstat(root_path)
    except OSError as error:
        raise fail("seed_root_read") from error
    if (
        not stat.S_ISDIR(root_info.st_mode)
        or os.path.realpath(root_path) != root_path
        or (expected_owner is not None and (root_info.st_uid, root_info.st_gid) != expected_owner)
    ):
        raise fail("seed_root_type")

    entries: list[dict[str, object]] = []
    required = 0

    def visit(directory: str, parent: str) -> None:
        nonlocal required
        try:
            names = sorted(os.listdir(directory))
        except OSError as error:
            raise fail("seed_directory_read") from error
        for name in names:
            if name in ("", ".", "..") or "/" in name or "\x00" in name:
                raise fail("seed_path")
            relative = f"{parent}/{name}" if parent else name
            if exclude_work and relative == "_work":
                continue
            if relative == "_work" or relative.startswith("_work/"):
                raise fail("seed_work_path")
            path = os.path.join(directory, name)
            try:
                info = os.lstat(path)
            except OSError as error:
                raise fail("seed_entry_read") from error
            if expected_owner is not None and (info.st_uid, info.st_gid) != expected_owner:
                raise fail("seed_owner")
            mode = stat.S_IMODE(info.st_mode)
            if mode & 0o6000:
                raise fail("seed_privileged_mode")
            if stat.S_ISDIR(info.st_mode):
                if mode & 0o022:
                    raise fail("seed_directory_mode")
                entry: dict[str, object] = {
                    "path": relative,
                    "kind": "directory",
                    "mode": mode,
                    "size": 0,
                    "allocated_bytes": allocated_bytes(info, "directory"),
                }
                entries.append(entry)
                required += int(entry["allocated_bytes"])
                visit(path, relative)
            elif stat.S_ISREG(info.st_mode):
                if info.st_nlink != 1 or mode & 0o022:
                    raise fail("seed_file_mode_or_link")
                digest = file_digest(path, info)
                entry = {
                    "path": relative,
                    "kind": "file",
                    "mode": mode,
                    "size": info.st_size,
                    "allocated_bytes": allocated_bytes(info, "file"),
                    "sha256": digest,
                }
                entries.append(entry)
                required += int(entry["allocated_bytes"])
            elif stat.S_ISLNK(info.st_mode):
                try:
                    target = os.readlink(path)
                except OSError as error:
                    raise fail("seed_symlink_read") from error
                validate_symlink(relative, target)
                entry = {
                    "path": relative,
                    "kind": "symlink",
                    "mode": mode,
                    "size": len(os.fsencode(target)),
                    "allocated_bytes": allocated_bytes(info, "symlink"),
                    "target": target,
                }
                entries.append(entry)
                required += int(entry["allocated_bytes"])
            else:
                raise fail("seed_special_file")
            if len(entries) > MAX_ENTRIES:
                raise fail("seed_entry_limit")

    visit(root_path, "")
    entries.sort(key=lambda entry: str(entry["path"]))
    if exclude_work:
        entries = [entry for entry in entries if entry["path"] != "_work" and not str(entry["path"]).startswith("_work/")]
        required = sum(int(entry["allocated_bytes"]) for entry in entries)
    return entries, required


def identity_document(
    entries: list[dict[str, object]],
    required: int,
    runner_version: str,
    archive_url: str,
    archive_sha256: str,
) -> dict[str, object]:
    if not runner_version or not archive_url:
        raise fail("seed_identity")
    if len(archive_sha256) != 64 or any(ch not in "0123456789abcdef" for ch in archive_sha256):
        raise fail("seed_archive_sha256")
    if required + HOME_ALLOWANCE_BYTES > HOME_LIMIT_BYTES:
        raise fail("seed_home_budget")
    return {
        "schema_version": SCHEMA_VERSION,
        "runner_version": runner_version,
        "archive_url": archive_url,
        "archive_sha256": archive_sha256,
        "home_limit_bytes": HOME_LIMIT_BYTES,
        "home_allowance_bytes": HOME_ALLOWANCE_BYTES,
        "entry_limit": MAX_ENTRIES,
        "required_allocated_bytes": required,
        "entries": entries,
    }


def compare_entries(
    actual: list[dict[str, object]],
    expected: list[dict[str, object]],
    *,
    compare_allocation: bool = True,
) -> bool:
    fields = ("path", "kind", "mode", "size", "allocated_bytes", "sha256", "target")
    if not compare_allocation:
        fields = tuple(field for field in fields if field != "allocated_bytes")
    return [tuple(entry.get(field) for field in fields) for entry in actual] == [
        tuple(entry.get(field) for field in fields) for entry in expected
    ]


def validate_manifest_entries(entries: list[object]) -> None:
    previous = ""
    for raw in entries:
        if not isinstance(raw, dict):
            raise fail("manifest_entry")
        path = raw.get("path")
        kind = raw.get("kind")
        mode = raw.get("mode")
        size = raw.get("size")
        allocated = raw.get("allocated_bytes")
        if (
            not isinstance(path, str)
            or not path
            or path.startswith("/")
            or path == "_work"
            or path.startswith("_work/")
            or posixpath.normpath(path) != path
            or any(part in ("", ".", "..") for part in path.split("/"))
            or path <= previous
            or kind not in ("directory", "file", "symlink")
            or type(mode) is not int
            or mode < 0
            or mode > 0o777
            or mode & 0o6000
            or (kind != "symlink" and mode & 0o022)
            or type(size) is not int
            or size < 0
            or type(allocated) is not int
            or allocated < 4096
        ):
            raise fail("manifest_entry")
        previous = path
        if kind == "file":
            digest = raw.get("sha256")
            if not isinstance(digest, str) or len(digest) != 64 or any(ch not in "0123456789abcdef" for ch in digest):
                raise fail("manifest_entry_digest")
        elif kind == "symlink":
            target = raw.get("target")
            if not isinstance(target, str):
                raise fail("manifest_entry_link")
            validate_symlink(path, target)
            if size != len(os.fsencode(target)):
                raise fail("manifest_entry_link")
        elif size != 0:
            raise fail("manifest_entry_directory")


def load_manifest(path: str, *, require_root: bool = True) -> dict[str, object]:
    descriptor = -1
    try:
        flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
        descriptor = os.open(path, flags)
        info = os.fstat(descriptor)
        if (
            not stat.S_ISREG(info.st_mode)
            or info.st_nlink != 1
            or (require_root and info.st_uid != 0)
            or info.st_mode & 0o222
        ):
            raise fail("manifest_file_mode")
        if info.st_size > MAX_MANIFEST_BYTES:
            raise fail("manifest_size")
        with os.fdopen(descriptor, "rb", closefd=True) as source:
            descriptor = -1
            raw = source.read(MAX_MANIFEST_BYTES + 1)
        if len(raw) > MAX_MANIFEST_BYTES:
            raise fail("manifest_size")

        def unique_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
            result: dict[str, object] = {}
            for key, value in pairs:
                if key in result:
                    raise fail("manifest_duplicate_key")
                result[key] = value
            return result

        document = json.loads(raw, object_pairs_hook=unique_pairs)
    except SeedError:
        raise
    except (OSError, ValueError, TypeError) as error:
        raise fail("manifest_read") from error
    finally:
        if descriptor >= 0:
            os.close(descriptor)
    if (
        not isinstance(document, dict)
        or type(document.get("schema_version")) is not int
        or document.get("schema_version") != SCHEMA_VERSION
    ):
        raise fail("manifest_schema")
    if document.get("home_limit_bytes") != HOME_LIMIT_BYTES or document.get("home_allowance_bytes") != HOME_ALLOWANCE_BYTES:
        raise fail("manifest_budget")
    entries = document.get("entries")
    if not isinstance(entries, list) or len(entries) > MAX_ENTRIES:
        raise fail("manifest_entries")
    validate_manifest_entries(entries)
    runner_version = document.get("runner_version")
    archive_url = document.get("archive_url")
    archive_sha256 = document.get("archive_sha256")
    required = document.get("required_allocated_bytes")
    if (
        not isinstance(runner_version, str)
        or not runner_version
        or not isinstance(archive_url, str)
        or not archive_url.startswith("https://github.com/actions/runner/releases/download/v")
        or not archive_url.endswith(
            f"/v{runner_version}/actions-runner-linux-x64-{runner_version}.tar.gz"
        )
        or not isinstance(archive_sha256, str)
        or len(archive_sha256) != 64
        or any(ch not in "0123456789abcdef" for ch in archive_sha256)
        or type(required) is not int
        or required < 0
        or required + HOME_ALLOWANCE_BYTES > HOME_LIMIT_BYTES
        or sum(int(entry["allocated_bytes"]) for entry in entries) != required
    ):
        raise fail("manifest_identity")
    return document


def checked_directory(path: str, uid: int, gid: int, reason: str) -> os.stat_result:
    try:
        info = os.lstat(path)
    except OSError as error:
        raise fail(reason) from error
    mode = stat.S_IMODE(info.st_mode)
    if (
        not stat.S_ISDIR(info.st_mode)
        or (info.st_uid, info.st_gid) != (uid, gid)
        or mode & 0o022
        or mode & 0o700 != 0o700
    ):
        raise fail(reason)
    if os.path.realpath(path) != path:
        raise fail(reason)
    return info


def check_home_mounts(home: str, work: str, mountinfo_path: str) -> None:
    try:
        with open(mountinfo_path, encoding="utf-8") as source:
            targets = mount_targets(source.read())
    except (OSError, UnicodeError) as error:
        raise fail("mountinfo_read") from error
    if home not in targets or work not in targets:
        raise fail("home_mount_missing")
    nested = {target for target in targets if target.startswith(home.rstrip("/") + "/")}
    if nested != {work}:
        raise fail("home_mount_layout")


def copy_file(source: str, target: str, entry: dict[str, object]) -> None:
    source_flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    target_flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        source_fd = os.open(source, source_flags)
    except OSError as error:
        raise fail("seed_copy_open") from error
    try:
        target_fd = os.open(target, target_flags, 0o600)
    except OSError as error:
        os.close(source_fd)
        raise fail("seed_copy_open") from error
    digest = hashlib.sha256()
    size = 0
    with os.fdopen(source_fd, "rb", closefd=True) as src, os.fdopen(target_fd, "wb", closefd=True) as dst:
        before = os.fstat(src.fileno())
        if not stat.S_ISREG(before.st_mode):
            raise fail("seed_copy_type")
        while True:
            chunk = src.read(CHUNK_BYTES)
            if not chunk:
                break
            dst.write(chunk)
            digest.update(chunk)
            size += len(chunk)
        after = os.fstat(src.fileno())
        if (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        ):
            raise fail("seed_copy_changed")
        if size != entry["size"] or digest.hexdigest() != entry["sha256"]:
            raise fail("seed_copy_digest")
        os.fchmod(dst.fileno(), int(entry["mode"]))
        os.fsync(dst.fileno())


def create_stage(seed: str, stage: str, entries: list[dict[str, object]]) -> None:
    directories = [entry for entry in entries if entry["kind"] == "directory"]
    files = [entry for entry in entries if entry["kind"] == "file"]
    links = [entry for entry in entries if entry["kind"] == "symlink"]
    for entry in directories:
        target = os.path.join(stage, str(entry["path"]))
        os.mkdir(target, 0o700)
    for entry in files:
        relative = str(entry["path"])
        copy_file(os.path.join(seed, relative), os.path.join(stage, relative), entry)
    for entry in links:
        relative = str(entry["path"])
        target = str(entry["target"])
        os.symlink(target, os.path.join(stage, relative))
    for entry in reversed(directories):
        os.chmod(os.path.join(stage, str(entry["path"])), int(entry["mode"]))


def remove_tree(path: str) -> None:
    def retry_writable(function, failed_path: str, error_info) -> None:
        if os.path.islink(failed_path):
            os.unlink(failed_path)
            return
        os.chmod(failed_path, 0o700)
        function(failed_path)

    if os.path.islink(path):
        os.unlink(path)
    elif os.path.isdir(path):
        shutil.rmtree(path, onerror=retry_writable)
    elif os.path.lexists(path):
        os.unlink(path)


def initialize(
    seed: str,
    home: str,
    manifest_path: str,
    *,
    expected_uid: int = 1000,
    expected_gid: int = 1000,
    expected_parent_uid: int = 0,
    expected_parent_gid: int = 0,
    expected_seed_uid: int = 0,
    expected_seed_gid: int = 0,
    require_mounts: bool = True,
    require_root_manifest: bool = True,
    mountinfo_path: str = "/proc/self/mountinfo",
    available_bytes: int | None = None,
) -> None:
    if os.geteuid() != expected_uid:
        raise fail("runner_uid")
    checked_directory(os.path.dirname(home), expected_parent_uid, expected_parent_gid, "home_parent")
    checked_directory(home, expected_uid, expected_gid, "home_owner")
    work = os.path.join(home, "_work")
    checked_directory(work, expected_uid, expected_gid, "work_owner")
    if require_mounts:
        check_home_mounts(home, work, mountinfo_path)

    names = set(os.listdir(home))
    if names != {"_work"}:
        raise fail("home_not_fresh")

    manifest = load_manifest(manifest_path, require_root=require_root_manifest)
    expected_entries = manifest["entries"]
    if not isinstance(expected_entries, list):
        raise fail("manifest_entries")
    checked_directory(seed, expected_seed_uid, expected_seed_gid, "seed_owner")
    source_entries, source_required = inventory(
        seed, expected_owner=(expected_seed_uid, expected_seed_gid)
    )
    if not compare_entries(source_entries, expected_entries):
        raise fail("seed_manifest_mismatch")
    if source_required != manifest.get("required_allocated_bytes"):
        raise fail("seed_budget_mismatch")
    if source_required + HOME_ALLOWANCE_BYTES > HOME_LIMIT_BYTES:
        raise fail("seed_home_budget")

    if available_bytes is None:
        try:
            space = os.statvfs(home)
        except OSError as error:
            raise fail("home_capacity") from error
        available_bytes = space.f_bavail * (space.f_frsize or space.f_bsize)
    if available_bytes < source_required + HOME_ALLOWANCE_BYTES:
        raise fail("home_capacity")

    stage_name = f".velnor-runner-seed.{os.getpid()}.{secrets.token_hex(8)}"
    stage = os.path.join(home, stage_name)
    published: list[str] = []
    try:
        os.mkdir(stage, 0o700)
        create_stage(seed, stage, expected_entries)
        staged_entries, _ = inventory(stage)
        if not compare_entries(staged_entries, expected_entries, compare_allocation=False):
            raise fail("stage_manifest_mismatch")
        top_level = sorted({str(entry["path"]).split("/", 1)[0] for entry in expected_entries})
        for name in top_level:
            if name in ("_work", ".", "..") or "/" in name:
                raise fail("seed_top_level")
            target = os.path.join(home, name)
            if os.path.lexists(target):
                raise fail("home_destination_exists")
            os.rename(os.path.join(stage, name), target)
            published.append(target)
        os.rmdir(stage)
        published_entries, _ = inventory(home, exclude_work=True)
        if not compare_entries(published_entries, expected_entries, compare_allocation=False):
            raise fail("home_manifest_mismatch")
        listener = os.path.join(home, "bin", "Runner.Listener")
        listener_entry = next((entry for entry in expected_entries if entry.get("path") == "bin/Runner.Listener"), None)
        if listener_entry is None or listener_entry.get("kind") != "file" or not int(listener_entry["mode"]) & 0o111:
            raise fail("listener_manifest")
        if file_digest(listener, os.lstat(listener)) != listener_entry.get("sha256"):
            raise fail("listener_digest")
    except SeedError:
        for path in reversed(published):
            remove_tree(path)
        if os.path.lexists(stage):
            remove_tree(stage)
        raise
    except OSError as error:
        for path in reversed(published):
            remove_tree(path)
        if os.path.lexists(stage):
            remove_tree(stage)
        raise fail("home_publish") from error


def write_manifest(args: argparse.Namespace) -> None:
    entries, required = inventory(args.root, expected_owner=(0, 0))
    document = identity_document(entries, required, args.runner_version, args.archive_url, args.archive_sha256)
    print(
        f"Runner seed: entries={len(entries)} allocated_bytes={required} "
        f"allowance_bytes={HOME_ALLOWANCE_BYTES} home_limit_bytes={HOME_LIMIT_BYTES}",
        file=sys.stderr,
    )
    encoded = canonical_json(document)
    if args.output == "-":
        sys.stdout.buffer.write(encoded)
        return
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_CLOEXEC", 0)
    descriptor = os.open(args.output, flags, 0o600)
    with os.fdopen(descriptor, "wb", closefd=True) as output:
        output.write(encoded)
        output.flush()
        os.fsync(output.fileno())


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    manifest = commands.add_parser("manifest")
    manifest.add_argument("--root", required=True)
    manifest.add_argument("--runner-version", required=True)
    manifest.add_argument("--archive-url", required=True)
    manifest.add_argument("--archive-sha256", required=True)
    manifest.add_argument("--output", default="-")
    initialize_parser = commands.add_parser("initialize")
    initialize_parser.add_argument("--seed", default="/opt/runner")
    initialize_parser.add_argument("--home", default=RUNNER_HOME)
    initialize_parser.add_argument("--manifest", default="/usr/share/velnor/runner-home-manifest.json")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.command == "manifest":
            write_manifest(args)
        else:
            initialize(args.seed, args.home, args.manifest)
    except (SeedError, OSError, ValueError) as error:
        reason = str(error) or "operation_failed"
        print(f"The Runner home volume failed: {reason}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
