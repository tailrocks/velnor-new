#!/usr/bin/env python3
"""Check that every file exempt from the no-absolute-local-paths rule is digest-frozen.

The exempt paths are listed in .alint.yml under no-absolute-local-paths. Each one must
be recorded with the same sha256 (and size) in a retained manifest: capture-inventory.json,
first/freeze.json or SHA256SUMS. Anything else is an ordinary file and must be rewritten.

Usage: check-digest-frozen-evidence.py [repo-root] [alint-config]
"""
import hashlib
import json
import os
import re
import sys

RULE_ID = "no-absolute-local-paths"
MANIFESTS = [
    "docs/implemented/evidence/freshness-2026-10-20/capture-inventory.json",
    "docs/implemented/evidence/freshness-2026-10-20/first/freeze.json",
    "docs/implemented/evidence/freshness-2026-10-24/SHA256SUMS",
]


def exempt_paths(config):
    """Return the exclude entries of the no-absolute-local-paths rule, minus the rule's own file."""
    out, on, in_exclude = [], False, False
    for line in open(config, encoding="utf-8"):
        line = line.rstrip("\n")
        if line == f"  - id: {RULE_ID}":
            on = True
            continue
        if on and re.match(r"^  - id:", line):
            break
        if on and line == "      exclude:":
            in_exclude = True
            continue
        if in_exclude:
            m = re.match(r'^        - "(.+)"$', line)
            if m:
                out.append(m.group(1))
            else:
                in_exclude = False
    return [p for p in out if p != ".alint.yml"]


def manifest_entries(root, rel_manifest):
    """Map manifest-relative names to (sha256, size or None) for one manifest file."""
    path = os.path.join(root, rel_manifest)
    if not os.path.exists(path):
        return {}
    entries = {}
    if rel_manifest.endswith(".json"):
        data = json.load(open(path, encoding="utf-8"))
        files = data.get("files", data)
        for name, meta in files.items():
            if isinstance(meta, dict) and "sha256" in meta:
                entries[name] = (meta["sha256"], meta.get("size"))
    else:
        for line in open(path, encoding="utf-8"):
            parts = line.split(None, 1)
            if len(parts) == 2:
                name = parts[1].strip()
                if name.startswith("./"):
                    name = name[2:]
                entries[name] = (parts[0], None)
    return entries


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest(), os.path.getsize(path)


def check(root, paths):
    failures = []
    cache = {m: manifest_entries(root, m) for m in MANIFESTS}
    for p in paths:
        full = os.path.join(root, p)
        if not os.path.isfile(full):
            failures.append(f"{p}: exempt path does not exist")
            continue
        digest, size = sha256_of(full)
        recorded = False
        for m in MANIFESTS:
            base = os.path.dirname(m)
            name = os.path.relpath(p, base)
            if name.startswith(".."):
                continue
            entry = cache[m].get(name)
            if entry is None:
                continue
            want_sha, want_size = entry
            if want_sha == digest and (want_size is None or want_size == size):
                recorded = True
                break
        if not recorded:
            failures.append(f"{p}: sha256 not recorded with a matching value in a retained manifest")
    return failures


def main():
    root = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.getcwd()
    config = sys.argv[2] if len(sys.argv) > 2 else os.path.join(root, ".alint.yml")
    paths = exempt_paths(config)
    if len(paths) != len(set(paths)):
        print("duplicate exempt paths")
        return 1
    failures = check(root, paths)
    for f in failures:
        print("FAIL", f)
    if failures:
        print(f"digest-frozen evidence check: {len(failures)} of {len(paths)} exempt paths unrecorded")
        return 1
    print(f"digest-frozen evidence check: OK ({len(paths)} exempt paths, all recorded)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
