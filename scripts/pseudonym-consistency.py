#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import csv
import io
import json
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


def disagreements(root):
    output = subprocess.run(
        ["git", "-C", str(root), "grep", "-n", "-I", "-E", "-z", "-e", "example-repo-[0-9]+", "--", "."],
        capture_output=True,
    ).stdout
    seen = defaultdict(lambda: defaultdict(set))
    for line in output.split(b"\n"):
        path, _, rest = line.partition(b"\0")
        _number, _, row = rest.partition(b"\0")
        if not row:
            continue
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
    try:
        text = (root / path).read_text()
        return list(owner_values(json.loads(text)) if path.endswith(".json") else csv_owner_values(text))
    except (OSError, ValueError):
        return []


def unscrubbed_owners(root):
    listed = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z", "--", *(f"{base}/*.{kind}" for base in EVIDENCE_DIRS for kind in ("json", "csv"))],
        capture_output=True,
    ).stdout
    found = []
    for raw in listed.split(b"\0"):
        path = raw.decode()
        for number, key, value in evidence_values(root, path) if path else []:
            org = value.split("/", 1)[0]
            if org not in PROJECT_OWNERS and not ALIAS.fullmatch(org):
                found.append((path, number, key))
    return found


def main():
    root = Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], check=True, capture_output=True, text=True).stdout.strip())
    found = disagreements(root)
    if found:
        print(f"{len(found)} record(s) carry different example-repo-N pseudonyms in different files; one repository must map to one pseudonym everywhere:")
        for key, names in found[:MAX_REPORTED]:
            print(f"  {key.decode().replace(":", " ")}: " + ", ".join(sorted(f"{name.decode()} in {len(paths)} file(s)" for name, paths in names.items())))
        return 1
    owners = unscrubbed_owners(root)
    if owners:
        print(f"{len(owners)} owner, account, login or org value(s) in evidence files are not example-org or example-account pseudonyms or the project org; replace each with an alias:")
        for path, number, field in owners[:MAX_REPORTED]:
            print(f"  {path} row {number} field {field}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
