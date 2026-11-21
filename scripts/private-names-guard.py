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
MAX_RUN = 6
MAX_REPORTED = 50
TOKEN = re.compile(rb"[a-z0-9_.\-]+")
ALNUM = re.compile(rb"[a-z0-9]+")
TRIM = b".-_"


def digest(word):
    return hashlib.sha256(SALT + word).hexdigest()


def load_hashes(path=HASH_FILE):
    return frozenset(path.read_text().split())


class Matcher:
    def __init__(self, hashes):
        self.hashes = hashes
        self.words = {}
        self.pairs = {}

    def word(self, word):
        known = self.words.get(word)
        if known is None:
            parts = [(m.start(), m.end()) for m in ALNUM.finditer(word)]
            spans = (
                word[parts[first][0] : parts[last][1]]
                for first in range(len(parts))
                for last in range(first, min(len(parts), first + MAX_RUN))
            )
            known = digest(word) in self.hashes or any(digest(span) in self.hashes for span in spans)
            self.words[word] = known
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
    for entry in output.decode("utf-8", "replace").split("\0"):
        meta, tab, path = entry.partition("\t")
        if tab:
            entries.setdefault(path, meta.split(" ")[1])
    return entries


def text_of(path):
    if not path.is_file() or path.is_symlink():
        return None
    data = path.read_bytes()
    return None if b"\0" in data else data


def scan(root, hashes):
    matcher = Matcher(hashes)
    entries = tracked_files(root)
    by_object = {}
    labels = {}
    found = []
    for number, (path, obj) in enumerate(entries.items(), 1):
        if path in EXEMPT:
            continue
        flagged = bool(matcher.flagged_lines(path.encode("utf-8")))
        labels[path] = f"ls-files line {number}" if flagged else path
        if flagged:
            found.append(labels[path])
        by_object.setdefault(obj, []).append(path)
    for paths in by_object.values():
        text = text_of(root / paths[0])
        lines = matcher.flagged_lines(text) if text is not None else []
        for path in paths:
            found.extend(f"{labels[path]}:{line}" for line in lines)
    return sorted(found)


def main():
    root = Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], check=True, capture_output=True, text=True).stdout.strip())
    found = scan(root, load_hashes())
    if found:
        print(f"{len(found)} finding(s): a tracked path or file line contains a private downstream name; use example-org and example-repo placeholders. A path is named by its line number in `git ls-files`:")
        print("\n".join(found[:MAX_REPORTED]))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
