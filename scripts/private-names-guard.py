#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import hashlib
import multiprocessing
import os
import re
import subprocess
import sys
from pathlib import Path

SALT = b"velnor-private-name-guard-v1:"
HASH_FILE = Path(__file__).with_name("private-names-hashes.txt")
MAX_REPORTED = 50
CHUNK = re.compile(rb"[a-z0-9_.\-]+(?:/[a-z0-9_.\-]+)*")
DROP = b"._-"
SYMLINK_MODE = "120000"
GITLINK_MODE = "160000"
TRIM = b".-_"


def sha(candidate):
    return hashlib.sha256(SALT + candidate).digest()


def join(first, second):
    return first.strip(TRIM) + b"/" + second.strip(TRIM)


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


SHARED = {}
PARALLEL_FROM = 20_000


def search(task):
    length, low, high = task
    digests = SHARED["digests"]
    sha256 = hashlib.sha256
    salt = SALT
    substrings = {
        word[start : start + length]
        for word in SHARED["words"][low:high]
        if len(word) >= length
        for start in range(len(word) - length + 1)
    }
    return {candidate for candidate in substrings if sha256(salt + candidate).digest() in digests}


class Matcher:
    def __init__(self, hashes):
        self.digests = frozenset(bytes.fromhex(hex_digest) for hex_digest in hashes)
        self.lengths = sorted(set(hashes.values()))
        self.bad = set()
        self.seen = set()

    def learn(self, chunks):
        fresh = set(chunks) - self.seen
        self.seen |= fresh
        split = {chunk: chunk.split(b"/") for chunk in fresh}
        by_word = {}
        for parts in split.values():
            for part in parts:
                by_word.setdefault(part.translate(None, DROP), set()).add(part)
        names = self.names_in(list(by_word))
        bad_tokens = {part for word, parts in by_word.items() if any(name in word for name in names) for part in parts}
        pairs = {
            pair
            for parts in split.values()
            for pair in map(join, parts, parts[1:])
            if len(pair) in self.lengths
        }
        bad_pairs = {pair for pair in pairs if sha(pair) in self.digests}
        for chunk, parts in split.items():
            if not bad_tokens.isdisjoint(parts) or (bad_pairs and not bad_pairs.isdisjoint(map(join, parts, parts[1:]))):
                self.bad.add(chunk)

    def names_in(self, words):
        SHARED.update(digests=self.digests, words=words)
        workers = min(os.cpu_count() or 1, 8)
        step = -(-len(words) // workers) or 1
        tasks = [(length, low, low + step) for length in self.lengths for low in range(0, len(words), step)]
        if workers == 1 or len(words) < PARALLEL_FROM or "fork" not in multiprocessing.get_all_start_methods():
            return set().union(*map(search, tasks))
        context = multiprocessing.get_context("fork")
        children = []
        for index in range(workers):
            part = tasks[index::workers]
            receiver, sender = context.Pipe(False)
            child = context.Process(target=lambda: sender.send(set().union(*map(search, part))))
            child.start()
            sender.close()
            children.append((child, receiver, part))
        names = set()
        for child, receiver, part in children:
            try:
                names |= receiver.recv()
            except EOFError:
                names |= set().union(*map(search, part))
            child.join()
        return names

    def touches(self, chunks):
        return not self.bad.isdisjoint(chunks)

    def lines(self, text):
        return [
            number
            for number, line in enumerate(text.lower().split(b"\n"), 1)
            if not self.bad.isdisjoint(CHUNK.findall(line))
        ]

    def flagged_lines(self, text):
        self.learn(CHUNK.findall(text.lower()))
        return self.lines(text)


def tracked_files(root):
    output = subprocess.run(["git", "-C", str(root), "ls-files", "-s", "-z"], check=True, capture_output=True).stdout
    entries = {}
    for line in output.split(b"\0"):
        meta, tab, path = line.partition(b"\t")
        if tab:
            mode, obj, _stage = meta.split(b" ")
            entries.setdefault(path, (mode.decode(), obj.decode()))
    return entries


def blobs(root, objects):
    objects = sorted(objects)
    output = subprocess.run(
        ["git", "-C", str(root), "cat-file", "--batch"],
        input="".join(f"{obj}\n" for obj in objects).encode(),
        check=True,
        capture_output=True,
    ).stdout
    texts = {}
    position = 0
    for obj in objects:
        newline = output.index(b"\n", position)
        header = output[position:newline].split(b" ")
        if len(header) != 3:
            raise SystemExit(f"git cat-file could not read blob {obj}; the guard cannot scan what it cannot read")
        size = int(header[2])
        texts[obj] = output[newline + 1 : newline + 1 + size]
        position = newline + 2 + size
    return texts


def scan(root, hashes):
    matcher = Matcher(hashes)
    listed = tracked_files(root)
    paths = {path: number for number, path in enumerate(listed, 1)}
    by_object = {}
    for path in paths:
        mode, obj = listed[path]
        if mode == GITLINK_MODE:
            continue
        by_object.setdefault((mode == SYMLINK_MODE, obj), []).append(path)
    contents = blobs(root, {obj for _symlink, obj in by_object})
    texts = {
        key: contents[key[1]].lower() for key in by_object if key[0] or b"\0" not in contents[key[1]]
    }
    seen = {name: set(CHUNK.findall(text)) for name, text in [*texts.items(), *((path, path.lower()) for path in paths)]}
    matcher.learn(set().union(*seen.values()))
    found = []
    labels = {}
    for path, number in paths.items():
        flagged = matcher.touches(seen[path])
        labels[path] = f"ls-files line {number}" if flagged else path.decode("utf-8", "backslashreplace")
        if flagged:
            found.append(labels[path])
    for key, text in texts.items():
        lines = matcher.lines(text) if matcher.touches(seen[key]) else []
        places = ["target"] if key[0] and lines else lines
        found.extend(f"{labels[path]}:{place}" for path in by_object[key] for place in places)
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
