#!/usr/bin/env python3
import hashlib
import json
import os
import re
import sys

RULE_IDS = ["no-absolute-local-paths", "no-local-temp-paths"]
MANIFESTS = [
    "docs/implemented/evidence/freshness-2026-10-20/capture-inventory.json",
    "docs/implemented/evidence/freshness-2026-10-20/first/freeze.json",
    "docs/implemented/evidence/freshness-2026-10-24/SHA256SUMS",
]


PINNED_LOGS = {
    "docs/implemented/evidence/freshness-2026-10-13/main-run-38316930881-plan-job-115035687366.log": (
        "acc1236ed3ab0fd94030bedcf3e496e9790a8dcce0bcb8001b31b263f4295f91",
        347778,
    ),
    "docs/implemented/evidence/freshness-2026-10-25/runner-plan.log": (
        "13e24e5b054cb423fdfad5b60a6bc84d852d992679efa933219433d3d454ae43",
        665901,
    ),
}


def exempt_paths(config):
    out = []
    for rule_id in RULE_IDS:
        on, in_exclude = False, False
        for line in open(config, encoding="utf-8"):
            line = line.rstrip("\n")
            if line == f"  - id: {rule_id}":
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
        if p in PINNED_LOGS:
            if PINNED_LOGS[p] != (digest, size):
                failures.append(f"{p}: differs from the sha256 and bytes its claim record pins")
            continue
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
