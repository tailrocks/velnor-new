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
OWNER_FIELD = re.compile(r"(?:^|_)(?:owner|account|login|org|organization)s?$")
REPOSITORY_FIELD = re.compile(r"(?:^|_)(?:repo|repos|repository|repositories)$")
WORD_BREAK = re.compile(r"(?<=[a-z0-9])(?=[A-Z])|[^A-Za-z0-9]+")
LIST_DELIMITERS = re.compile(r"[;,|]")
BOM = "\ufeff"
EVIDENCE_TABLES = {".csv": ",", ".tsv": "\t"}
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


def repository_names(fields):
    own = [(key, PSEUDONYM.findall(str(value).encode())) for key, value in fields if not owner_key(key)]
    named = [names for key, names in own if names and REPOSITORY_FIELD.search(WORD_BREAK.sub("_", key).lower())]
    return (named or [names for _key, names in own if names] or [[]])[0]


def json_rows(node):
    if isinstance(node, dict):
        fields = []
        pairs = list(scalars(node))
        for key, value in pairs:
            literal = value if key == "number" and isinstance(value, str) and value.isdigit() else json.dumps(value)
            fields.append(f"{json.dumps(key)}: {literal}")
        yield ", ".join(fields).encode(), repository_names(pairs)
        for value in node.values():
            yield from json_rows(value)
    elif isinstance(node, list):
        for value in node:
            yield from json_rows(value)


def reject_duplicate_keys(pairs):
    keys = [key for key, _value in pairs]
    if len(set(keys)) != len(keys):
        raise ValueError("duplicate JSON key")
    return dict(pairs)


def decode_text(data):
    text = data.decode().removeprefix(BOM)
    if "\0" in text:
        raise ValueError("NUL byte")
    return text


def parse_json(data):
    return json.loads(decode_text(data), object_pairs_hook=reject_duplicate_keys)


def json_document(root, path):
    return parse_json(index_blob(root, path))


def pseudonym_files(root):
    done = subprocess.run(
        ["git", "-C", str(root), "grep", "--cached", "-a", "-l", "-z", "-E", "-e", "example-repo-[0-9]+", "--", "."],
        capture_output=True,
    )
    if done.returncode not in (0, 1):
        raise subprocess.CalledProcessError(done.returncode, done.args, done.stdout, done.stderr)
    return sorted(filter(None, done.stdout.split(b"\0")))


def file_rows(root, path):
    if path.lower().endswith(b".json"):
        try:
            return list(json_rows(json_document(root, path)))
        except (ValueError, RecursionError):
            pass
    return [(row, PSEUDONYM.findall(row)) for row in index_blob(root, path).split(b"\n")]


def disagreements(root):
    seen = defaultdict(lambda: defaultdict(set))
    for path in pseudonym_files(root):
        for row, names in file_rows(root, path):
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
        if path.lower().endswith(b".json"):
            try:
                json_document(root, path)
            except (ValueError, RecursionError):
                broken.append(os.fsdecode(path))
    return broken


def owner_key(key):
    return bool(OWNER_FIELD.search(WORD_BREAK.sub("_", key).lower()))


def leaves(node):
    if isinstance(node, dict):
        for value in node.values():
            yield from leaves(value)
    elif isinstance(node, list):
        for value in node:
            yield from leaves(value)
    else:
        yield node


def owner_values(node, number=None):
    if isinstance(node, dict):
        number = node.get("number", number)
        for key, value in node.items():
            if owner_key(key):
                for leaf in leaves(value):
                    yield number, key, leaf
            else:
                yield from owner_values(value, number)
    elif isinstance(node, list):
        for value in node:
            yield from owner_values(value, number)


def table_owner_values(text, delimiter):
    rows = csv.reader(io.StringIO(text, newline=""), delimiter=delimiter)
    header = next(rows, [])
    columns = [(index, name) for index, name in enumerate(header) if owner_key(name)]
    for row in rows:
        for index, name in columns:
            if index < len(row):
                yield None, name, row[index]


def acceptable(value):
    if value is None or isinstance(value, bool):
        return True
    if not isinstance(value, str):
        return False
    for part in LIST_DELIMITERS.split(value):
        account = part.strip()
        if ACCOUNT.fullmatch(account):
            org = account.split("/", 1)[0]
            if org not in PROJECT_OWNERS and not ALIAS.fullmatch(org):
                return False
    return True


def evidence_values(root, path):
    data = index_blob(root, os.fsencode(path))
    suffix = Path(path.lower()).suffix
    if suffix == ".json":
        return list(owner_values(parse_json(data)))
    return list(table_owner_values(decode_text(data), EVIDENCE_TABLES[suffix]))


def describe(path):
    return os.fsencode(path).decode("utf-8", "backslashreplace")


def unscrubbed_owners(root):
    listed = git_bytes(root, "ls-files", "-z", "--", *EVIDENCE_DIRS)
    found = []
    for raw in filter(None, listed.split(b"\0")):
        path = os.fsdecode(raw)
        if Path(path.lower()).suffix not in (".json", *EVIDENCE_TABLES):
            continue
        try:
            values = evidence_values(root, path)
        except (OSError, ValueError, csv.Error, RecursionError, subprocess.CalledProcessError):
            found.append((path, None, "unparseable"))
            continue
        for number, key, value in values:
            if not acceptable(value):
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
        print(f"{len(owners)} owner, account, login or org value(s) in evidence files are not example-org or example-account aliases or the project org, or the file cannot be parsed; one original owner must always map to the same alias, so replace each with its alias or fix the file. Private names are decided by scripts/private-names-guard.py, not by this check:")
        for path, number, field in owners[:MAX_REPORTED]:
            print(f"  {describe(path)} row {number} field {field}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
