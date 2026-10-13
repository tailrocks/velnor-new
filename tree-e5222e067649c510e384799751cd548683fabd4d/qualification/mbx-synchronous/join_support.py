"""Shared bounded byte and JSON checks for synchronous evidence joins."""

import hashlib
import json
from pathlib import Path
import re
import stat
from typing import Any

SCOPE = "exact_synchronous_fixture_v1"
MAX_BYTES = 512 * 1024 * 1024
MAX_REPORT_BYTES = 8 * 1024 * 1024
MAX_INVENTORY_BYTES = 64 * 1024 * 1024
MAX_ARTIFACTS = 100_000
SHA = re.compile(r"[0-9a-f]{64}\Z")
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\Z")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def keys(value: Any, expected: str, name: str) -> None:
    require(type(value) is dict and set(value) == set(expected.split()),
            name + " schema differs")


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def regular(path: Path, bound: int = MAX_BYTES) -> bytes:
    require(path.is_absolute() and path == path.resolve(strict=True),
            "absolute canonical artifact path required")
    named = path.lstat()
    require(stat.S_ISREG(named.st_mode), "regular artifact required")
    require(named.st_size <= bound, "artifact exceeds bound")
    with path.open("rb") as stream:
        opened = stream.fileno()
        import os
        metadata = os.fstat(opened)
        require((metadata.st_dev, metadata.st_ino) == (named.st_dev, named.st_ino),
                "artifact changed during open")
        data = stream.read(bound + 1)
    require(len(data) == named.st_size and len(data) <= bound,
            "artifact size changed or exceeds bound")
    return data


def artifact(value: Any, bound: int = MAX_BYTES) -> bytes:
    keys(value, "path size sha256", "artifact")
    require(type(value["path"]) is str and type(value["size"]) is int
            and 0 <= value["size"] <= bound, "invalid artifact path/size")
    require(type(value["sha256"]) is str and SHA.fullmatch(value["sha256"]),
            "invalid artifact digest")
    data = regular(Path(value["path"]), bound)
    require(len(data) == value["size"] and sha(data) == value["sha256"],
            "artifact bytes differ")
    return data


def object_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def document(data: bytes) -> Any:
    def invalid(value: str) -> None:
        raise ValueError("nonfinite JSON value: " + value)

    return json.loads(data, object_pairs_hook=object_pairs, parse_constant=invalid)
