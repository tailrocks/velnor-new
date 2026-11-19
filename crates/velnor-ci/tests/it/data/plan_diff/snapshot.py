#!/usr/bin/env python3
"""Writes the reduced repository a plan_diff case plans against.

Usage: snapshot.py <clone> <base|-> <head> <out dir> [path ...]

The paths are kept too (the files a local case edits).

Every file is stored with a .fixture suffix, so that no fixture is a manifest or a source file of this
repository. The out dir gets common/ (files equal at both commits), base-only/ and head-only/ (files that
differ or exist on one side). Manifests, .velnor/config.toml, mise configurations and build.rs
paths are kept whole or as markers; a .rs file is reduced to the lines the legacy source scan can
react to (plus three lines after each), because that scan is all the selection reads of a
source file. record.sh checks the helper plans the reduced repository the same as the real one.
"""
import os
import re
import subprocess
import sys

clone, base, head, out = sys.argv[1:5]
also = sys.argv[5:]
base = None if base == "-" else base

ANCHORS = r'CARGO_MANIFEST_DIR|current_dir|concat!'
DIRECT = (
    r'include(_str|_bytes)?!|' + ANCHORS + r'|\bpath\s*=\s*"|"[^"\n]*\.\.[^"\n]*"'
    r'|\b(read|read_to_string|read_dir|open|metadata|symlink_metadata|canonicalize|copy)\("'
)
ESCAPES = r'\bparent\b|ancestors'
TOKENS = re.compile(DIRECT + '|' + ESCAPES)
KEEP_WHOLE = re.compile(r'(^|/)(Cargo\.toml|mise\.toml|\.mise\.toml)$|^\.velnor/config\.toml$')


def git(*args):
    return subprocess.run(["git", "-C", clone, *args], capture_output=True, check=True).stdout


def tree(rev):
    if rev is None:
        return set()
    return {p.decode() for p in git("ls-tree", "-r", "--name-only", "-z", rev).split(b"\0") if p}


def blob(rev, path):
    if rev is None:
        return None
    r = subprocess.run(["git", "-C", clone, "show", f"{rev}:{path}"], capture_output=True)
    return r.stdout if r.returncode == 0 else None


def stub(text):
    lines = text.decode("utf-8", "replace").split("\n")
    keep = set()
    for i, line in enumerate(lines):
        if TOKENS.search(line):
            keep.update(range(i, min(i + 4, len(lines))))
    return ("\n".join(lines[i] for i in sorted(keep)) + "\n").encode()


def reduce(path, data):
    if KEEP_WHOLE.search(path):
        return data
    if path.endswith("build.rs"):
        return b"// build script\n"
    if path.endswith(".rs"):
        return stub(data)
    return b"// content\n" if len(data) > 4096 else data


changed = set()
if base:
    out_names = git("diff", "-z", "--name-only", "--no-renames", f"{base}...{head}", "--")
    changed = {p.decode() for p in out_names.split(b"\0") if p}
head_tree, base_tree = tree(head), tree(base)
relevant = set(changed) | set(also)
for path in head_tree | base_tree:
    if KEEP_WHOLE.search(path) or path.endswith("build.rs"):
        relevant.add(path)
def grep(pattern):
    found = git("grep", "-l", "-z", "-E", pattern, head, "--", "*.rs").split(b"\0")
    return {name.decode().split(":", 1)[1] for name in found if name}


def package_of(path):
    parts = path.split("/")
    for n in range(len(parts) - 1, -1, -1):
        directory = "/".join(parts[:n])
        if (directory + "/Cargo.toml" if directory else "Cargo.toml") in head_tree:
            return directory
    return ""


relevant |= grep(DIRECT)
anchored = {package_of(path) for path in grep(ANCHORS)}
relevant |= {path for path in grep(ESCAPES) if package_of(path) in anchored}

for path in sorted(relevant):
    a, b = blob(base, path), blob(head, path)
    a = None if a is None else reduce(path, a)
    b = None if b is None else reduce(path, b)
    if base is None or (a is not None and a == b and path not in changed):
        sides = [("common", b)]
    else:
        if a is not None and b is not None:
            a, b = a + b"// base\n", b + b"// head\n"
        sides = [("base-only", a), ("head-only", b)]
    for name, data in sides:
        if data is None:
            continue
        target = os.path.join(out, name, path) + ".fixture"
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(target, "wb") as f:
            f.write(data)
