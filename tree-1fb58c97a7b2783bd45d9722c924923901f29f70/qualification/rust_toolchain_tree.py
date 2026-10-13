"""Safe extraction and exact Velnor Rust install-tree projection."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import stat
import tarfile
from pathlib import Path, PurePosixPath

MAX_ARCHIVE_BYTES = 1024 * 1024 * 1024
MAX_ENTRY_BYTES = 256 * 1024 * 1024
MAX_TOTAL_BYTES = 4 * 1024 * 1024 * 1024
MAX_ENTRIES = 100_000


class QualificationError(Exception):
    """A source, extraction, install, or receipt check failed."""


def digest_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digest_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def safe_path(value: str, allow_trailing_slash: bool = False) -> PurePosixPath:
    if (
        not value
        or len(value.encode()) > 4096
        or value.startswith("/")
        or any(byte < 0x21 or byte > 0x7E or byte in (ord("\\"), ord(":")) for byte in value.encode())
    ):
        raise QualificationError("unsafe Rust archive path")
    text = value[:-1] if allow_trailing_slash and value.endswith("/") else value
    parts = text.split("/")
    if not text or any(part in ("", ".", "..") for part in parts) or len(parts) > 256:
        raise QualificationError("unsafe Rust archive path component")
    return PurePosixPath(*parts)


def ensure_parent(root: Path, relative: PurePosixPath) -> Path:
    parent = root
    for part in relative.parts[:-1]:
        parent = parent / part
        try:
            parent.mkdir()
        except FileExistsError:
            if not parent.is_dir() or parent.is_symlink():
                raise QualificationError("Rust archive parent is not an owned directory")
    return parent / relative.parts[-1]


def extract_archive(archive_path: Path, destination: Path) -> None:
    if archive_path.stat().st_size > MAX_ARCHIVE_BYTES:
        raise QualificationError("Rust archive exceeds the input byte limit")
    destination.mkdir()
    with tarfile.open(archive_path, mode="r:xz") as archive:
        members = archive.getmembers()
        if len(members) > MAX_ENTRIES:
            raise QualificationError("Rust archive exceeds the entry limit")
        seen: set[str] = set()
        links: list[tuple[PurePosixPath, str]] = []
        directories: list[tuple[PurePosixPath, int]] = []
        total = 0
        for member in members:
            relative = safe_path(member.name, member.isdir())
            key = relative.as_posix()
            if key in seen:
                raise QualificationError("Rust archive contains a duplicate path")
            seen.add(key)
            if member.isdir():
                path = destination.joinpath(*relative.parts)
                path.mkdir(parents=True, exist_ok=True)
                directories.append((relative, member.mode))
            elif member.isfile():
                if member.size > MAX_ENTRY_BYTES:
                    raise QualificationError("Rust archive entry exceeds the byte limit")
                total += member.size
                if total > MAX_TOTAL_BYTES:
                    raise QualificationError("Rust archive exceeds the extracted byte limit")
                path = ensure_parent(destination, relative)
                source = archive.extractfile(member)
                if source is None:
                    raise QualificationError("Rust archive file has no payload")
                flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
                with os.fdopen(os.open(path, flags, 0o600), "wb") as output, source:
                    shutil.copyfileobj(source, output, 1024 * 1024)
                if path.stat().st_size != member.size:
                    raise QualificationError("Rust archive file was truncated")
                path.chmod(member.mode & 0o777)
            elif member.issym():
                target = member.linkname
                total += len(target.encode())
                if total > MAX_TOTAL_BYTES:
                    raise QualificationError("Rust archive exceeds the extracted byte limit")
                if not target or len(target.encode()) > 4096 or target.startswith("/") or "\\" in target:
                    raise QualificationError("Rust archive symlink target is unsafe")
                links.append((relative, target))
            else:
                raise QualificationError("Rust archive contains a hardlink or special entry")
        for relative, target in links:
            os.symlink(target, ensure_parent(destination, relative))
        for relative, _ in links:
            path = destination.joinpath(*relative.parts)
            try:
                resolved = path.resolve(strict=True)
            except OSError as error:
                raise QualificationError("Rust archive symlink is dangling") from error
            if not resolved.is_file() or not resolved.is_relative_to(destination.resolve()):
                raise QualificationError("Rust archive symlink escapes its archive")
        for relative, mode in reversed(directories):
            destination.joinpath(*relative.parts).chmod(mode & 0o777)


def component_root(extracted: Path) -> Path:
    inventory = extracted / "components"
    if inventory.is_file() and not inventory.is_symlink():
        return extracted
    candidates = [
        path for path in extracted.iterdir()
        if path.is_dir() and not path.is_symlink()
        and (path / "components").is_file() and not (path / "components").is_symlink()
    ]
    if len(candidates) != 1:
        raise QualificationError("Rust archive component root is ambiguous")
    return candidates[0]


def component_inventory(root: Path) -> set[str]:
    path = root / "components"
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 1024 * 1024:
        raise QualificationError("Rust component inventory is not a bounded regular file")
    names = path.read_text(encoding="utf-8").splitlines()
    if not names or len(names) != len(set(names)) or any(
        not re.fullmatch(r"[A-Za-z0-9_.-]+", name) for name in names
    ):
        raise QualificationError("Rust component inventory is invalid")
    return set(names)


def merge_component(source_root: Path, component: str, prefix: Path) -> None:
    if component not in component_inventory(source_root):
        raise QualificationError(f"Rust component is absent: {component}")
    component_dir = source_root / component
    if component_dir.is_symlink() or not component_dir.is_dir():
        raise QualificationError("Rust component root is not a regular directory")
    manifest_path = component_dir / "manifest.in"
    if manifest_path.is_symlink() or not manifest_path.is_file() or manifest_path.stat().st_size > 1024 * 1024:
        raise QualificationError("Rust component manifest is not a bounded regular file")
    entries: set[str] = set()
    parsed: list[tuple[str, PurePosixPath]] = []
    for line in manifest_path.read_text(encoding="utf-8").splitlines():
        kind, separator, path_text = line.partition(":")
        if not separator or kind not in ("file", "dir"):
            raise QualificationError("Rust component manifest row is invalid")
        relative = safe_path(path_text)
        key = relative.as_posix()
        if key in entries:
            raise QualificationError("Rust component manifest repeats a path")
        entries.add(key)
        source = component_dir.joinpath(*relative.parts)
        for parent in relative.parents[:-1]:
            parent_path = component_dir.joinpath(*parent.parts)
            if parent_path.is_symlink() or not parent_path.is_dir():
                raise QualificationError("Rust component manifest parent is not a directory")
        if (kind == "dir" and (not source.is_dir() or source.is_symlink())) or (
            kind == "file" and source.is_dir()
        ):
            raise QualificationError("Rust component manifest type differs from payload")
        parsed.append((kind, relative))
    if not parsed:
        raise QualificationError("Rust component payload is empty")
    for _, relative in sorted(parsed, key=lambda entry: entry[1].as_posix()):
        move_payload(component_dir.joinpath(*relative.parts), prefix.joinpath(*relative.parts), prefix)


def move_payload(source: Path, destination: Path, prefix: Path) -> None:
    relative = destination.relative_to(prefix)
    current = prefix
    for part in relative.parts[:-1]:
        current = current / part
        if current.exists() or current.is_symlink():
            if not current.is_dir() or current.is_symlink():
                raise QualificationError("Rust component destination parent is not a directory")
        else:
            current.mkdir()
    if not destination.exists() and not destination.is_symlink():
        os.rename(source, destination)
        return
    if source.is_dir() and not source.is_symlink() and destination.is_dir() and not destination.is_symlink():
        for child in sorted(source.iterdir(), key=lambda path: path.name):
            move_payload(child, destination / child.name, prefix)
        source.rmdir()
        return
    raise QualificationError("Rust component payload paths collide")


def tree_entries(root: Path) -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    for parent, dirnames, filenames in os.walk(root, followlinks=False):
        base = Path(parent)
        dirnames[:] = sorted(dirnames)
        for name in sorted([*dirnames, *filenames]):
            path = base / name
            relative = path.relative_to(root).as_posix()
            mode = path.lstat().st_mode
            if stat.S_ISLNK(mode):
                target = os.readlink(path)
                resolved = path.resolve(strict=True)
                if os.path.isabs(target) or "\\" in target or not resolved.is_relative_to(root.resolve()) or not resolved.is_file():
                    raise QualificationError("installed Rust symlink escapes its prefix")
                entries.append({"path": relative, "kind": "symlink", "target": target})
            elif stat.S_ISDIR(mode):
                entries.append({"path": relative, "kind": "directory"})
            elif stat.S_ISREG(mode):
                entries.append({"path": relative, "kind": "file", "sha256": digest_file(path), "executable": bool(mode & 0o111)})
            else:
                raise QualificationError("installed Rust tree contains a special file")
    return sorted(entries, key=lambda entry: str(entry["path"]))


def canonical_tree_sha256(entries: list[dict[str, object]]) -> str:
    payload = b"[" + b",".join(
        json.dumps(entry, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
        for entry in entries
    ) + b"]"
    return digest_bytes(payload)
