"""Shared constants and evidence primitives for Debian package tooling."""

from __future__ import annotations

import hashlib
import json
import re
import tarfile
from pathlib import Path
from typing import Any

PACKAGE_NAME = "velnor-host"
TOOL_VERSION = "3.8.0"
TARGETS = {
    "amd64": ("x86_64-unknown-linux-gnu", "Advanced Micro Devices X86-64"),
    "arm64": ("aarch64-unknown-linux-gnu", "AArch64"),
    "armhf": ("armv7-unknown-linux-gnueabihf", "ARM"),
    "i386": ("i686-unknown-linux-gnu", "Intel 80386"),
}
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
TREE_RE = re.compile(r"^[0-9a-f]{40}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
DEBIAN_UPSTREAM_RE = re.compile(r"^[0-9][A-Za-z0-9.+~]*$")
DEV_VERSION_RE = re.compile(
    r"^(?P<upstream>[0-9][A-Za-z0-9.+~]*)~dev(?P<sequence>[0-9]{4})"
    r"\+(?P<utc>[0-9]{14})\+g(?P<commit>[0-9a-f]{12})-(?P<revision>[1-9][0-9]*)$"
)


class EvidenceError(Exception):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise EvidenceError(f"cannot read JSON evidence {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise EvidenceError(f"JSON evidence is not an object: {path}")
    return value


def require_sha(value: Any, label: str, pattern: re.Pattern[str] = SHA256_RE) -> str:
    if not isinstance(value, str) or not pattern.fullmatch(value):
        raise EvidenceError(f"{label} must be a lowercase full-length hexadecimal digest")
    return value

def archive_member_sha256(archive_path: Path, wanted: str) -> str:
    matches: list[tarfile.TarInfo] = []
    with tarfile.open(archive_path, mode="r:*") as archive:
        for member in archive.getmembers():
            name = member.name
            while name.startswith("./"):
                name = name[2:]
            if name == wanted:
                matches.append(member)
        if len(matches) != 1 or not matches[0].isfile():
            raise EvidenceError(f"source archive must contain one regular {wanted}")
        stream = archive.extractfile(matches[0])
        if stream is None:
            raise EvidenceError(f"cannot read {wanted} from source archive")
        digest = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
        return digest.hexdigest()
