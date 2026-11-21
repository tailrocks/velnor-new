#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import csv
import io
import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

PSEUDONYM = re.compile(rb"example-repo-[0-9]+(?![0-9A-Za-z_])")
SHA = re.compile(rb"\b[0-9a-f]{40}\b")
NODE_ID = re.compile(rb"\bR_kgD[A-Za-z0-9_-]+")
NUMBER = re.compile(rb'^(?:\|\s*|\s*\{?\s*"number":\s*)?([0-9]+)\s*[,|]|"number":\s*([0-9]+)')
MAX_REPORTED = 50
ALIAS = re.compile(r"example-(?:org|account)(?:-[a-z0-9]+)*")
OWNER_FIELD = re.compile(r"(?:^|_)(?:owner|account|login|org|organization)$")
ACCOUNT = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*(?:/[A-Za-z0-9._-]+)?")
EVIDENCE_DIRS = ("docs/proposed", "docs/reviews")
PROJECT_OWNERS = {"tailrocks"}


def row_keys(row):
    keys = {b"node:" + node for node in NODE_ID.findall(row)}
    number = NUMBER.search(row)
    if number:
        record = number.group(1) or number.group(2)
        keys.update(b"record:" + record + b":" + sha for sha in SHA.findall(row))
    return keys


def git_bytes(root, *args):
    return subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True).stdout


def index_blob(root, path):
    return git_bytes(root, "cat-file", "blob", b":" + path)


def scalars(node):
    for key, value in node.items():
        if isinstance(value, list):
            for item in value:
                if not isinstance(item, (dict, list)):
                    yield key, item
        elif not isinstance(value, dict):
            yield key, value


def json_rows(node):
    if isinstance(node, dict):
        fields = []
        for key, value in scalars(node):
            literal = value if key == "number" and isinstance(value, str) and value.isdigit() else json.dumps(value)
            fields.append(f"{json.dumps(key)}: {literal}")
        yield ", ".join(fields).encode()
        for value in node.values():
            yield from json_rows(value)
    elif isinstance(node, list):
        for value in node:
            yield from json_rows(value)


def json_document(root, path):
    return json.loads(index_blob(root, path).decode())


def pseudonym_files(root):
    done = subprocess.run(
        ["git", "-C", str(root), "grep", "--cached", "-a", "-l", "-z", "-E", "-e", "example-repo-[0-9]+", "--", "."],
        capture_output=True,
    )
    if done.returncode not in (0, 1):
        raise subprocess.CalledProcessError(done.returncode, done.args, done.stdout, done.stderr)
    return sorted(filter(None, done.stdout.split(b"\0")))


def file_rows(root, path):
    if path.endswith(b".json"):
        try:
            return list(json_rows(json_document(root, path)))
        except (ValueError, RecursionError):
            pass
    return index_blob(root, path).split(b"\n")


def disagreements(root):
    seen = defaultdict(lambda: defaultdict(set))
    for path in pseudonym_files(root):
        for row in file_rows(root, path):
            names = PSEUDONYM.findall(row)
            if not names:
                continue
            for key in row_keys(row):
                seen[key][names[0]].add(path)
    labelled = defaultdict(lambda: defaultdict(set))
    for key, names in seen.items():
        if len(names) > 1:
            label = key.rsplit(b":", 1)[0] if key.startswith(b"record:") else key
            for name, paths in names.items():
                labelled[label][name] |= paths
    return sorted(labelled.items())


def broken_json(root):
    broken = []
    for path in pseudonym_files(root):
        if path.endswith(b".json"):
            try:
                json_document(root, path)
            except (ValueError, RecursionError):
                broken.append(os.fsdecode(path))
    return broken


def owner_values(node, number=None):
    if isinstance(node, dict):
        number = node.get("number", number)
        for key, value in node.items():
            if isinstance(value, str) and OWNER_FIELD.search(key) and ACCOUNT.fullmatch(value):
                yield number, key, value
            yield from owner_values(value, number)
    elif isinstance(node, list):
        for value in node:
            yield from owner_values(value, number)


def csv_owner_values(text):
    rows = csv.DictReader(io.StringIO(text))
    fields = [name for name in rows.fieldnames or [] if OWNER_FIELD.search(name)]
    for row in rows:
        for name in fields:
            if ACCOUNT.fullmatch(row.get(name) or ""):
                yield None, name, row[name]


def evidence_values(root, path):
    text = index_blob(root, os.fsencode(path)).decode()
    if "\0" in text:
        raise ValueError(path)
    return list(owner_values(json.loads(text)) if path.endswith(".json") else csv_owner_values(text))


def describe(path):
    return os.fsencode(path).decode("utf-8", "backslashreplace")


def unscrubbed_owners(root):
    patterns = [f"{base}/*.{kind}" for base in EVIDENCE_DIRS for kind in ("json", "csv")]
    listed = git_bytes(root, "ls-files", "-z", "--", *patterns)
    found = []
    for raw in filter(None, listed.split(b"\0")):
        path = os.fsdecode(raw)
        try:
            values = evidence_values(root, path)
        except (OSError, ValueError, csv.Error, RecursionError, subprocess.CalledProcessError):
            found.append((path, None, "unparseable"))
            continue
        for number, key, value in values:
            org = value.split("/", 1)[0]
            if org not in PROJECT_OWNERS and not ALIAS.fullmatch(org):
                found.append((path, number, key))
    return found


def main():
    root = Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], check=True, capture_output=True, text=True).stdout.strip())
    broken = broken_json(root)
    if broken:
        print(f"{len(broken)} tracked JSON file(s) naming example-repo-N pseudonyms cannot be parsed, so their records cannot be compared; fix the syntax:")
        for path in broken[:MAX_REPORTED]:
            print(f"  {describe(path)}")
        return 1
    found = disagreements(root)
    if found:
        print(f"{len(found)} record(s) carry different example-repo-N pseudonyms in different files; one repository must map to one pseudonym everywhere:")
        for key, names in found[:MAX_REPORTED]:
            print(f"  {key.decode().replace(":", " ")}: " + ", ".join(sorted(f"{name.decode()} in {len(paths)} file(s)" for name, paths in names.items())))
        return 1
    owners = unscrubbed_owners(root)
    if owners:
        print(f"{len(owners)} owner, account, login or org value(s) in evidence files are not example-org or example-account pseudonyms or the project org, or the file cannot be parsed; replace each with an alias or fix the file:")
        for path, number, field in owners[:MAX_REPORTED]:
            print(f"  {describe(path)} row {number} field {field}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
