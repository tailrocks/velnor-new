#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
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


def main():
    root = Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], check=True, capture_output=True, text=True).stdout.strip())
    found = disagreements(root)
    if found:
        print(f"{len(found)} record(s) carry different example-repo-N pseudonyms in different files; one repository must map to one pseudonym everywhere:")
        for key, names in found[:MAX_REPORTED]:
            print(f"  {key.decode().replace(":", " ")}: " + ", ".join(sorted(f"{name.decode()} in {len(paths)} file(s)" for name, paths in names.items())))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
