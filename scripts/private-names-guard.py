#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import hashlib
import re
import subprocess
import sys
from pathlib import Path

SALT = b"velnor-private-name-guard-v1:"
HASH_FILE = Path(__file__).with_name("private-names-hashes.txt")
EXEMPT = frozenset({"docs/hard-decisions.md"})
MAX_REPORTED = 50
TOKEN = re.compile(rb"[a-z0-9_.\-]+")
NON_ALNUM = re.compile(rb"[^a-z0-9]+")
SYMLINK_MODE = "120000"
TRIM = b".-_"


def digest(word):
    return hashlib.sha256(SALT + word).hexdigest()


def normalize(name):
    name = name.lower()
    return name if "/" in name else re.sub(r"[^a-z0-9]+", "", name)


def entry(name):
    key = normalize(name).encode()
    return f"{digest(key)} {len(key)}"


def load_hashes(path=HASH_FILE):
    return dict(
        (digest_, int(length))
        for digest_, length in (line.split() for line in path.read_text().splitlines() if line.strip())
    )


class Matcher:
    def __init__(self, hashes):
        self.hashes = hashes
        self.lengths = sorted(set(hashes.values()))
        self.words = {}
        self.pairs = {}

    def word(self, token):
        known = self.words.get(token)
        if known is None:
            squeezed = NON_ALNUM.sub(b"", token)
            known = any(
                digest(squeezed[start : start + length]) in self.hashes
                for length in self.lengths
                for start in range(len(squeezed) - length + 1)
            )
            self.words[token] = known
        return known

    def pair(self, first, second):
        pair = first.strip(TRIM) + b"/" + second.strip(TRIM)
        known = self.pairs.get(pair)
        if known is None:
            known = digest(pair) in self.hashes
            self.pairs[pair] = known
        return known

    def flagged_lines(self, text):
        flagged = []
        for number, line in enumerate(text.lower().split(b"\n"), 1):
            previous = None
            for match in TOKEN.finditer(line):
                start, end = match.span()
                token = match.group()
                slashed = previous is not None and previous[1] + 1 == start and line[previous[1]] == 0x2F
                if self.word(token) or (slashed and self.pair(line[previous[0] : previous[1]], token)):
                    flagged.append(number)
                    break
                previous = (start, end)
        return flagged


def tracked_files(root):
    output = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-s", "-z"], check=True, capture_output=True
    ).stdout
    entries = {}
    for line in output.decode("utf-8", "replace").split("\0"):
        meta, tab, path = line.partition("\t")
        if tab:
            mode, obj, _stage = meta.split(" ")
            entries.setdefault(path, (mode, obj))
    return entries


def blob_of(root, obj):
    return subprocess.run(
        ["git", "-C", str(root), "cat-file", "blob", obj], check=True, capture_output=True
    ).stdout


def text_of(root, path, mode, obj):
    if mode == SYMLINK_MODE:
        return blob_of(root, obj)
    file = root / path
    if not file.is_file() or file.is_symlink():
        return None
    data = file.read_bytes()
    return None if b"\0" in data else data


def scan(root, hashes):
    matcher = Matcher(hashes)
    entries = tracked_files(root)
    by_object = {}
    labels = {}
    found = []
    for number, (path, (mode, obj)) in enumerate(entries.items(), 1):
        if path in EXEMPT:
            continue
        flagged = bool(matcher.flagged_lines(path.encode("utf-8")))
        labels[path] = f"ls-files line {number}" if flagged else path
        if flagged:
            found.append(labels[path])
        by_object.setdefault((mode == SYMLINK_MODE, obj), []).append((path, mode))
    for (symlink, obj), paths in by_object.items():
        first, mode = paths[0]
        text = text_of(root, first, mode, obj)
        lines = matcher.flagged_lines(text) if text is not None else []
        places = ["target"] if symlink and lines else lines
        for path, _mode in paths:
            found.extend(f"{labels[path]}:{place}" for place in places)
    return sorted(found)


def main():
    root = Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], check=True, capture_output=True, text=True).stdout.strip())
    found = scan(root, load_hashes())
    if found:
        print(f"{len(found)} finding(s): a tracked path, file line or symlink target contains a private downstream name; use example-org and example-repo placeholders. A path is named by its line number in `git ls-files`:")
        print("\n".join(found[:MAX_REPORTED]))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
