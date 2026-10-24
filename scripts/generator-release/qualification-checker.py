#!/usr/bin/env python3
"""Trusted-source checks for release qualification candidate output."""

from __future__ import annotations

import json
from pathlib import Path
import re
import stat
import sys

MAX_CANDIDATE_MANIFEST_BYTES = 8 * 1024 * 1024
MAX_WORKFLOW_FILE_BYTES = 8 * 1024 * 1024
MAX_WORKFLOW_TREE_BYTES = 64 * 1024 * 1024
MAX_WORKFLOW_FILES = 10_000
MAX_WORKFLOW_DEPTH = 128
EXPECTED_TARGETS = (
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
)
BOUND_FIELDS = (
    "VELNOR_ASSET_SHA256",
    "VELNOR_ASSET_URL",
    "VELNOR_RELEASE_COMMIT",
)


class CheckError(Exception):
    """A bounded qualification check failed."""


class DuplicateKeyError(ValueError):
    """A JSON object contains the same key more than once."""


def reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise DuplicateKeyError(f"duplicate object key {key!r}")
        result[key] = value
    return result


def reject_non_json_constant(value: str) -> None:
    raise ValueError(f"invalid JSON constant {value}")


def read_regular_file(path: Path, limit: int) -> bytes:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise CheckError(str(error)) from error
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
        raise CheckError(f"expected regular file no larger than {limit} bytes")
    try:
        contents = path.read_bytes()
    except OSError as error:
        raise CheckError(str(error)) from error
    if len(contents) > limit:
        raise CheckError(f"file grew beyond {limit} bytes while reading")
    return contents


def parse_json(path: Path, limit: int) -> object:
    contents = read_regular_file(path, limit)
    try:
        return json.loads(
            contents.decode("utf-8"),
            object_pairs_hook=reject_duplicate_keys,
            parse_constant=reject_non_json_constant,
        )
    except (UnicodeError, ValueError) as error:
        raise CheckError(f"malformed or duplicate-key JSON: {error}") from error


def validate_candidate_manifest(path: Path) -> dict[str, object]:
    manifest = parse_json(path, MAX_CANDIDATE_MANIFEST_BYTES)
    if not isinstance(manifest, dict):
        raise CheckError("candidate manifest does not match the release schema or repository")
    if type(manifest.get("schema")) is not int or manifest["schema"] != 1:
        raise CheckError("candidate manifest does not match the release schema or repository")
    if manifest.get("repository") != "tailrocks/velnor-new":
        raise CheckError("candidate manifest does not match the release schema or repository")
    version = manifest.get("version")
    if not isinstance(version, str) or not re.fullmatch(
        r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", version
    ):
        raise CheckError("candidate manifest version is malformed")
    commit = manifest.get("commit")
    if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise CheckError("candidate manifest source commit is malformed")
    targets = manifest.get("targets")
    if not isinstance(targets, list) or len(targets) != len(EXPECTED_TARGETS):
        raise CheckError("candidate manifest target inventory is malformed")
    for item, expected_target in zip(targets, EXPECTED_TARGETS, strict=True):
        if not isinstance(item, dict):
            raise CheckError(
                "candidate manifest does not match the release schema or target inventory"
            )
        target = item.get("target")
        digest = item.get("sha256")
        artifact = item.get("artifact")
        expected_url = (
            f"https://github.com/tailrocks/velnor-new/releases/download/v{version}/"
            f"velnor-actions-{version}-{expected_target}"
        )
        if (
            target != expected_target
            or not isinstance(digest, str)
            or not re.fullmatch(r"[0-9a-f]{64}", digest)
            or artifact != expected_url
        ):
            raise CheckError(
                "candidate manifest does not match the release schema or target inventory"
            )
    return manifest


def collect_regular_files(root: Path, files: list[Path], depth: int = 0) -> None:
    if depth > MAX_WORKFLOW_DEPTH:
        raise CheckError(
            f"generated workflow tree exceeds {MAX_WORKFLOW_DEPTH} directory levels"
        )
    try:
        entries = sorted(root.iterdir(), key=lambda entry: entry.name)
    except OSError as error:
        raise CheckError(str(error)) from error
    for path in entries:
        try:
            mode = path.lstat().st_mode
        except OSError as error:
            raise CheckError(str(error)) from error
        if stat.S_ISLNK(mode):
            continue
        if stat.S_ISDIR(mode):
            collect_regular_files(path, files, depth + 1)
        elif stat.S_ISREG(mode):
            if len(files) == MAX_WORKFLOW_FILES:
                raise CheckError(
                    f"generated workflow tree exceeds {MAX_WORKFLOW_FILES} regular files"
                )
            files.append(path)


def is_binding_marker(line: str) -> bool:
    body = line.lstrip(" \t")
    return any(
        body.startswith(field)
        and len(body) > len(field)
        and body[len(field)] in " \t:"
        for field in BOUND_FIELDS
    )


def parse_binding_line(line: str) -> tuple[str, str, str]:
    indent_length = len(line) - len(line.lstrip(" \t"))
    indent = line[:indent_length]
    body = line[indent_length:]
    if ": " not in body:
        raise ValueError
    key, value = body.split(": ", 1)
    if not key or not value or any(character.isspace() for character in value):
        raise ValueError
    return indent, key, value


def rust_lines(text: str) -> list[str]:
    if not text:
        return []
    lines = text.split("\n")
    ends_with_line_feed = lines[-1] == ""
    if ends_with_line_feed:
        lines.pop()
    return [
        line[:-1]
        if line.endswith("\r") and (index < len(lines) - 1 or ends_with_line_feed)
        else line
        for index, line in enumerate(lines)
    ]


def validate_file_bindings(
    path: Path, text: str, by_url: dict[str, tuple[str, str]], commit: str
) -> int:
    lines = rust_lines(text)
    count = 0
    index = 0
    while index < len(lines):
        if not is_binding_marker(lines[index]):
            index += 1
            continue
        if index + len(BOUND_FIELDS) > len(lines):
            raise CheckError(f"incomplete binding in {path}")
        try:
            parsed = [parse_binding_line(line) for line in lines[index : index + 3]]
        except ValueError as error:
            raise CheckError(f"incomplete binding in {path}") from error
        if tuple(item[1] for item in parsed) != BOUND_FIELDS:
            raise CheckError(f"unpaired binding field in {path}")
        if any(item[0] != parsed[0][0] for item in parsed):
            raise CheckError(f"binding fields use different scopes in {path}")
        digest, url, source = (item[2] for item in parsed)
        target = by_url.get(url)
        if target is None:
            raise CheckError(f"unknown release URL in {path}")
        target_name, expected_digest = target
        if digest != expected_digest:
            raise CheckError(f"wrong digest for target {target_name} in {path}")
        if source != commit:
            raise CheckError(f"wrong source commit in {path}")
        count += 1
        index += len(BOUND_FIELDS)
    return count


def validate_output_bindings(manifest_path: Path, workflow_root: Path) -> None:
    manifest = validate_candidate_manifest(manifest_path)
    commit = manifest["commit"]
    targets = manifest["targets"]
    if not isinstance(commit, str) or not isinstance(targets, list) or not targets:
        raise CheckError("candidate manifest or generated workflow tree is incomplete")
    try:
        root_mode = workflow_root.lstat().st_mode
    except OSError as error:
        raise CheckError("candidate manifest or generated workflow tree is incomplete") from error
    if not stat.S_ISDIR(root_mode):
        raise CheckError("candidate manifest or generated workflow tree is incomplete")
    by_url: dict[str, tuple[str, str]] = {}
    for item in targets:
        if not isinstance(item, dict):
            raise CheckError("candidate manifest or generated workflow tree is incomplete")
        url = item.get("artifact")
        target = item.get("target")
        digest = item.get("sha256")
        if not isinstance(url, str) or not isinstance(target, str) or not isinstance(digest, str):
            raise CheckError("candidate manifest or generated workflow tree is incomplete")
        if url in by_url:
            raise CheckError("candidate manifest contains duplicate release URLs")
        by_url[url] = (target, digest)
    files: list[Path] = []
    collect_regular_files(workflow_root, files)
    total_bytes = 0
    bindings = 0
    for path in sorted(files):
        try:
            contents = read_regular_file(path, MAX_WORKFLOW_FILE_BYTES)
        except CheckError as error:
            raise CheckError(f"cannot read generated workflow {path}: {error}") from error
        total_bytes += len(contents)
        if total_bytes > MAX_WORKFLOW_TREE_BYTES:
            raise CheckError(
                f"generated workflow tree exceeds {MAX_WORKFLOW_TREE_BYTES} bytes"
            )
        try:
            text = contents.decode("utf-8")
        except UnicodeError as error:
            raise CheckError(f"cannot read generated workflow {path}: invalid UTF-8") from error
        bindings += validate_file_bindings(path, text, by_url, commit)
    if bindings == 0:
        raise CheckError("no ConsumerV1 download bindings were emitted")


def main(arguments: list[str]) -> int:
    if len(arguments) == 3 and arguments[1] == "manifest":
        try:
            validate_candidate_manifest(Path(arguments[2]))
        except CheckError as error:
            print(f"candidate manifest validation: {error}", file=sys.stderr)
            return 1
        return 0
    if len(arguments) == 4 and arguments[1] == "output-bindings":
        try:
            validate_output_bindings(Path(arguments[2]), Path(arguments[3]))
        except CheckError as error:
            print(f"candidate output binding mismatch: {error}", file=sys.stderr)
            return 1
        return 0
    print(
        "usage: qualification-checker.py manifest MANIFEST | "
        "output-bindings MANIFEST WORKFLOW_ROOT",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
