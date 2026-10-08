"""Parse and verify recorded build/package commands and their file evidence."""

from __future__ import annotations

import shlex
from pathlib import Path
from typing import Any

from debian_package_common import EvidenceError, require_sha, sha256_file


def verify_recorded_file(path_value: Any, expected_hash: Any, label: str) -> Path:
    if not isinstance(path_value, str) or not path_value:
        raise EvidenceError(f"{label} path must be a non-empty string")
    path = Path(path_value)
    if path.is_symlink() or not path.is_file():
        raise EvidenceError(f"{label} must be an existing regular file")
    digest = require_sha(expected_hash, f"{label} SHA-256")
    if sha256_file(path) != digest:
        raise EvidenceError(f"{label} bytes do not match the recorded SHA-256")
    return path

def command_tokens(path: Path, digest: Any, label: str) -> list[str]:
    path = verify_recorded_file(str(path), digest, label)
    try:
        tokens = shlex.split(path.read_text(encoding="utf-8"))
    except (UnicodeDecodeError, ValueError) as exc:
        raise EvidenceError(f"{label} is not a valid recorded command line") from exc
    if not tokens:
        raise EvidenceError(f"{label} is empty")
    return tokens

def require_nonempty_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise EvidenceError(f"{label} must be a non-empty string")
    return value

def command_flag_value(tokens: list[str], flag: str, label: str) -> str:
    values: list[str] = []
    for index, token in enumerate(tokens):
        if token == flag:
            if index + 1 >= len(tokens):
                raise EvidenceError(f"{label} is missing a value after {flag}")
            values.append(tokens[index + 1])
        elif token.startswith(flag + "="):
            values.append(token[len(flag) + 1 :])
    if len(values) != 1 or not values[0]:
        raise EvidenceError(f"{label} must contain exactly one {flag} value")
    return values[0]

def cargo_arguments(tokens: list[str], label: str, subcommand: str) -> list[str]:
    cargo_index = next((i for i, token in enumerate(tokens) if Path(token).name == "cargo"), None)
    if cargo_index is None:
        raise EvidenceError(f"{label} does not invoke Cargo")
    args = tokens[cargo_index + 1 :]
    if args and args[0].startswith("+"):
        args = args[1:]
    if not args or args[0] != subcommand:
        raise EvidenceError(f"{label} must invoke cargo {subcommand}")
    return args[1:]

def command_env_value(tokens: list[str], name: str, label: str) -> str:
    cargo_index = next((i for i, token in enumerate(tokens) if Path(token).name == "cargo"), None)
    if cargo_index is None:
        raise EvidenceError(f"{label} does not invoke Cargo")
    values = [token[len(name) + 1 :] for token in tokens[:cargo_index] if token.startswith(name + "=")]
    if len(values) != 1 or not values[0]:
        raise EvidenceError(f"{label} must record exactly one {name} before Cargo")
    return values[0]
