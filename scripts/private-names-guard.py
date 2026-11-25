#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import hashlib
import html
import itertools
import json
import multiprocessing
import os
import re
import subprocess
import sys
import tempfile
import unicodedata
from pathlib import Path
from urllib.parse import unquote_to_bytes

SALT = b"velnor-private-name-guard-v1:"
HASH_FILE = Path(__file__).with_name("private-names-hashes.txt")
MAX_REPORTED = 50
CHUNK = re.compile(rb"[a-z0-9_.\-]+(?:/[a-z0-9_.\-]+)*")
DROP = b"._-"
SYMLINK_MODE = "120000"
GITLINK_MODE = "160000"
TRIM = b".-_"
GIT_SUFFIX = b".git"
ENCODED = re.compile(rb"%[0-9a-f]{2}|\\u[0-9a-f]{4}|\\x[0-9a-f]{2}|\\/|&#?[a-z0-9]+;|[\x80-\xff]")
ESCAPE = re.compile(r"\\u([0-9a-f]{4})|\\x([0-9a-f]{2})|(\\/)")
BLANKS = re.compile(rb"[ \t\r\f\v]+")
NON_ASCII = re.compile(r"[^\x00-\x7f]+")
IGNORED_CATEGORIES = frozenset({"Cf", "Mn", "Me"})
MAX_PERCENT_ROUNDS = 4
EXEMPT_PATHS = frozenset({b"docs/hard-decisions.md", b".velnor/runner.lock"})
MARKUP = re.compile(rb"\]\([^)\n]*\)|\]\[[^\]\n]*\]|</?[a-z][^>\n]*>|[*`~\[\]]+")
LFS_PREFIX = b"version https://git-lfs.github.com/spec/"
LFS_OID = re.compile(rb"^oid sha256:([0-9a-f]{64})\r?$", re.MULTILINE)
LFS_MAX_POINTER = 1024
LFS_BLOCK = 1 << 18
LFS_OVERLAP = 1024
MAX_SEEN = 2_000_000
UNFETCHED_LFS_FIXTURE = b"version https://git-lfs.github.com/spec/v1\noid sha256:8c271ba1c7127e3409055b635a6a2e2671c004ff9f4a507c2cdfd9e0fb3bf90b\nsize 7\n"


def sha(candidate):
    return hashlib.sha256(SALT + candidate).digest()


def join(first, second):
    return first.strip(TRIM) + b"/" + second.removesuffix(GIT_SUFFIX).strip(TRIM)


def digest(word):
    return hashlib.sha256(SALT + word).hexdigest()


def normalize(name):
    name = name.lower()
    if "/" in name:
        first, _slash, second = name.partition("/")
        return join(first.encode(), second.encode()).decode()
    return re.sub(r"[^a-z0-9]+", "", name)


def entry(name):
    key = normalize(name).encode()
    return f"{digest(key)} {len(key)}"


def load_hashes(path=HASH_FILE):
    return dict(
        (digest_, int(length))
        for digest_, length in (line.split() for line in path.read_text().splitlines() if line.strip())
    )


SHARED = {}
PARALLEL_FROM = 200_000


def search(task):
    length, low, high = task
    digests = SHARED["digests"]
    sha256 = hashlib.sha256
    salt = SALT
    found = set()
    for word in SHARED["words"][low:high]:
        for start in range(len(word) - length + 1):
            candidate = word[start : start + length]
            if sha256(salt + candidate).digest() in digests:
                found.add(candidate)
    return found


class Matcher:
    def __init__(self, hashes):
        self.digests = frozenset(bytes.fromhex(hex_digest) for hex_digest in hashes)
        self.lengths = sorted(set(hashes.values()))
        self.bad = set()
        self.seen = set()

    def learn(self, chunks):
        if len(self.seen) > MAX_SEEN:
            self.seen.clear()
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
        if workers == 1 or sum(map(len, words)) < PARALLEL_FROM or "fork" not in multiprocessing.get_all_start_methods():
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
        found = (
            number
            for number, line in numbered(text.lower())
            if not self.bad.isdisjoint(CHUNK.findall(line))
        )
        return list(itertools.islice(found, MAX_REPORTED))

    def flagged_lines(self, text):
        self.learn(CHUNK.findall(text.lower()))
        return self.lines(text)

    def view_lines(self, views):
        return sorted({number for view in views for number in self.lines(view)})[:MAX_REPORTED]


def numbered(text):
    start = 0
    number = 1
    while True:
        end = text.find(b"\n", start)
        if end < 0:
            yield number, text[start:]
            return
        yield number, text[start:end]
        start = end + 1
        number += 1


def unpercent(line):
    for _round in range(MAX_PERCENT_ROUNDS):
        decoded = unquote_to_bytes(line)
        if decoded == line:
            break
        line = decoded
    return line


def fold(text):
    text = unicodedata.normalize("NFKD", unicodedata.normalize("NFKC", text).casefold())
    kept = "".join(char for char in text if unicodedata.category(char) not in IGNORED_CATEGORIES)
    return unicodedata.normalize("NFKC", kept).casefold()


def decode_line(line):
    text = unpercent(line).decode("utf-8", "replace")
    text = ESCAPE.sub(lambda found: "/" if found.group(3) else chr(int(found.group(1) or found.group(2), 16)), text)
    text = html.unescape(text).encode("utf-16-le", "surrogatepass").decode("utf-16-le", "replace")
    return NON_ASCII.sub(lambda run: fold(run.group()), text).casefold().encode().translate(None, b"\r\n")


def decoded(text):
    if not ENCODED.search(text):
        return text
    return b"\n".join(decode_line(line) if ENCODED.search(line) else line for line in text.split(b"\n"))


def spaced(text):
    return BLANKS.sub(b"", text)


def unmarked(text):
    return MARKUP.sub(b"", text)


def views(content):
    lowered = content.lower()
    bases = (lowered, lowered.replace(b"\0", b"")) if b"\0" in lowered else (lowered,)
    found = []
    for base in bases:
        for view in (base, decoded(base)):
            for candidate in (view, spaced(view), unmarked(view), spaced(unmarked(view))):
                if candidate not in found:
                    found.append(candidate)
    return tuple(found)


def tracked_files(root):
    output = subprocess.run(["git", "-C", str(root), "ls-files", "-s", "-z"], check=True, capture_output=True).stdout
    entries = {}
    for line in output.split(b"\0"):
        meta, tab, path = line.partition(b"\t")
        if tab:
            mode, obj, _stage = meta.split(b" ")
            entries.setdefault(path, (mode.decode(), obj.decode()))
    return entries


def stream_blobs(root, objects):
    with tempfile.TemporaryFile() as wanted:
        wanted.write("".join(f"{obj}\n" for obj in objects).encode())
        wanted.seek(0)
        process = subprocess.Popen(
            ["git", "-C", str(root), "cat-file", "--batch"], stdin=wanted, stdout=subprocess.PIPE
        )
        with process:
            for obj in objects:
                header = process.stdout.readline().rstrip(b"\n").split(b" ")
                if len(header) != 3:
                    raise SystemExit(f"git cat-file could not read blob {obj}; the guard cannot scan what it cannot read")
                content = process.stdout.read(int(header[2]))
                process.stdout.read(1)
                yield obj, content
        if process.returncode:
            raise SystemExit("git cat-file failed; the guard cannot scan what it cannot read")


def is_lfs_pointer(content):
    return content.startswith(LFS_PREFIX) and len(content) <= LFS_MAX_POINTER and content != UNFETCHED_LFS_FIXTURE


def lfs_object(root, content):
    pointer = LFS_OID.search(content)
    if not pointer:
        return None
    oid = pointer.group(1).decode()
    common = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "--path-format=absolute", "--git-common-dir"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    stored = Path(common, "lfs", "objects", oid[:2], oid[2:4], oid)
    return stored if stored.is_file() else None


def blocks(stored):
    with stored.open("rb") as stream:
        tail = b""
        while True:
            data = stream.read(LFS_BLOCK)
            if not data:
                break
            data = tail + data
            cut = data.rfind(b"\n") + 1
            if cut:
                tail = data[cut:]
                yield data[:cut]
            elif len(data) >= 2 * LFS_BLOCK:
                tail = data[-LFS_OVERLAP:]
                yield data
            else:
                tail = data
        if tail:
            yield tail


def payload_lines(matcher, stored):
    lines = set()
    offset = 0
    for block in blocks(stored):
        found_views = views(block)
        chunks = set().union(*(CHUNK.findall(view) for view in found_views))
        matcher.learn(chunks)
        if matcher.touches(chunks):
            lines.update(offset + number for number in matcher.view_lines(found_views))
        offset += block.count(b"\n")
    return lines


def blob_findings(root, matcher, key, content, names):
    found_views = views(content)
    chunks = set().union(*(CHUNK.findall(view) for view in found_views))
    matcher.learn(chunks)
    lines = set(matcher.view_lines(found_views)) if matcher.touches(chunks) else set()
    if not key[0] and is_lfs_pointer(content):
        stored = lfs_object(root, content)
        if stored is None:
            return [f"{name}:lfs-payload-unavailable" for name in names]
        lines |= payload_lines(matcher, stored)
    ordered = sorted(lines)[:MAX_REPORTED]
    places = ["target"] if key[0] and ordered else ordered
    return [f"{name}:{place}" for name in names for place in places]


def scan(root, hashes, only=None):
    matcher = Matcher(hashes)
    listed = tracked_files(root)
    paths = {
        path: number
        for number, path in enumerate(listed, 1)
        if (only is None or path in only) and path not in EXEMPT_PATHS
    }
    by_object = {}
    for path in paths:
        mode, obj = listed[path]
        if mode == GITLINK_MODE:
            continue
        by_object.setdefault((mode == SYMLINK_MODE, obj), []).append(path)
    flagged_paths = {}
    for path in paths:
        chunks = set().union(*(CHUNK.findall(view) for view in views(path)))
        matcher.learn(chunks)
        flagged_paths[path] = matcher.touches(chunks)
    found = []
    labels = {}
    for path, number in paths.items():
        labels[path] = f"ls-files line {number}" if flagged_paths[path] else path.decode("utf-8", "backslashreplace")
        if flagged_paths[path]:
            found.append(labels[path])
    keys = {}
    for key in by_object:
        keys.setdefault(key[1], []).append(key)
    for obj, content in stream_blobs(root, sorted(keys)):
        for key in keys[obj]:
            found.extend(blob_findings(root, matcher, key, content, [labels[path] for path in by_object[key]]))
    return sorted(found)


FULL_SCAN_TRIGGERS = frozenset({f"scripts/{HASH_FILE.name}".encode(), f"scripts/{Path(__file__).name}".encode()})


def changed_paths(root, environ):
    if environ.get("GITHUB_EVENT_NAME") != "pull_request":
        return None
    try:
        base = json.loads(Path(environ["GITHUB_EVENT_PATH"]).read_text())["pull_request"]["base"]["sha"]
        subprocess.run(["git", "-C", str(root), "fetch", "--quiet", "--no-tags", "--depth=1", "origin", base], check=True, capture_output=True)
        listed = subprocess.run(
            ["git", "-C", str(root), "diff", "--name-only", "-z", "--diff-filter=d", base, "HEAD"], check=True, capture_output=True
        ).stdout
    except (KeyError, TypeError, OSError, ValueError, subprocess.CalledProcessError):
        return None
    changed = set(filter(None, listed.split(b"\0")))
    return None if not changed.isdisjoint(FULL_SCAN_TRIGGERS) else changed


def run(root, hashes, environ):
    only = changed_paths(root, environ)
    found = scan(root, hashes, only)
    if found:
        print(f"{len(found)} finding(s): a tracked path, file line or symlink target contains a private downstream name; use example-org and example-repo placeholders. A path is named by its line number in `git ls-files`:")
        print("\n".join(found[:MAX_REPORTED]))
        return 1
    return 0


def main():
    root = Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], check=True, capture_output=True, text=True).stdout.strip())
    return run(root, load_hashes(), os.environ)


if __name__ == "__main__":
    sys.exit(main())
