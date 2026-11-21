#!/usr/bin/env python3
import hashlib
import json
import os
import re
import subprocess
import sys

RULE_IDS = ["no-absolute-local-paths", "no-local-temp-paths"]
MANIFESTS = [
    "docs/implemented/evidence/freshness-2026-10-20/capture-inventory.json",
    "docs/implemented/evidence/freshness-2026-10-20/first/freeze.json",
    "docs/implemented/evidence/freshness-2026-10-24/SHA256SUMS",
]

MANIFEST_PINS = {
    "docs/implemented/evidence/freshness-2026-10-20/capture-inventory.json": "5d94060363ac71580dcd22413799b2e80280aad8a43e9f4a4732d35c86d9dd57",
    "docs/implemented/evidence/freshness-2026-10-20/first/freeze.json": "4311b117aac03d3a3ea3e75c597a14f7c1bcaaf44b00cdb2f7d8d6c0158dcdbf",
    "docs/implemented/evidence/freshness-2026-10-24/SHA256SUMS": "f6533ec27eb072155de6996eae9031475840b2254ba423a9c9feab1fd51f6215",
}


PINNED_FILES = {
    "docs/implemented/evidence/freshness-2026-10-13/main-run-38316930881-plan-job-115035687366.log": (
        "acc1236ed3ab0fd94030bedcf3e496e9790a8dcce0bcb8001b31b263f4295f91",
        347778,
    ),
    "docs/implemented/evidence/freshness-2026-10-25/runner-plan.log": (
        "13e24e5b054cb423fdfad5b60a6bc84d852d992679efa933219433d3d454ae43",
        665901,
    ),
    "docs/reviews/phasec-registered-source-fixes-v31.json": (
        "2adf9a2f7c4e8c7251f902088424fb2096e6430c2885be8c85e2a1fd63a8cc75",
        9363,
    ),
}


def scalar(raw):
    raw = raw.strip()
    if raw.startswith('"'):
        m = re.match(r'^("(?:[^"\\]|\\.)*")\s*(?:#.*)?$', raw)
        if not m:
            raise ValueError(f"unsupported exclude entry: {raw}")
        return json.loads(m.group(1))
    if raw.startswith("'"):
        m = re.match(r"^'((?:[^']|'')*)'\s*(?:#.*)?$", raw)
        if not m:
            raise ValueError(f"unsupported exclude entry: {raw}")
        return m.group(1).replace("''", "'")
    value = re.sub(r"\s+#.*$", "", raw)
    if not value or value[0] in "[]{}&*!|>%@`," or ": " in value or value.endswith(":"):
        raise ValueError(f"unsupported exclude entry: {raw}")
    return value


def exclude_lines(config, rule_id):
    out = []
    on, exclude_indent = False, None
    for line in open(config, encoding="utf-8"):
        line = line.rstrip("\n")
        if line == f"  - id: {rule_id}":
            on = True
            continue
        if on and re.match(r"^  - id:", line):
            break
        if not on:
            continue
        m = re.match(r"^(\s*)exclude:\s*(.*)$", line)
        if m:
            if m.group(2) and not m.group(2).startswith("#"):
                raise ValueError(f"unsupported exclude syntax: {line.strip()}")
            exclude_indent = len(m.group(1))
            continue
        if exclude_indent is None:
            continue
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        indent = len(line) - len(line.lstrip())
        if indent < exclude_indent:
            exclude_indent = None
            continue
        m = re.match(r"^\s*- (.*)$", line)
        if not m:
            raise ValueError(f"unexpected line inside {rule_id} exclude: {line.strip()}")
        out.append(scalar(m.group(1)))
    return out


def resolved_excludes(config, rule_id):
    try:
        run = subprocess.run(
            ["alint", "explain", rule_id, "--config", config, "--format", "json"],
            capture_output=True, text=True, timeout=60, check=False,
        )
        if run.returncode != 0:
            raise ValueError(f"alint could not resolve {rule_id}: {run.stderr.strip() or run.stdout.strip()}")
        return json.loads(run.stdout)["paths"]["exclude"]
    except (OSError, subprocess.SubprocessError, KeyError, TypeError, json.JSONDecodeError) as err:
        raise ValueError(f"alint could not resolve {rule_id}: {err}")


def exempt_paths(config):
    out = []
    for rule_id in RULE_IDS:
        parsed = exclude_lines(config, rule_id)
        resolved = resolved_excludes(config, rule_id)
        if parsed != resolved:
            raise ValueError(
                f"{rule_id}: parsed {len(parsed)} exclude entries but alint resolves {len(resolved)}; refusing"
            )
        if len(parsed) != len(set(parsed)):
            raise ValueError(f"{rule_id}: duplicate exclude entries")
        out.extend(parsed)
    return list(dict.fromkeys(p for p in out if p != ".alint.yml"))


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
    cache = {}
    for m in MANIFESTS:
        full = os.path.join(root, m)
        if os.path.exists(full) and sha256_of(full)[0] != MANIFEST_PINS[m]:
            failures.append(f"{m}: manifest differs from its pinned sha256")
            cache[m] = {}
        else:
            cache[m] = manifest_entries(root, m)
    for p in paths:
        full = os.path.join(root, p)
        if not os.path.isfile(full):
            failures.append(f"{p}: exempt path does not exist")
            continue
        digest, size = sha256_of(full)
        if p in PINNED_FILES:
            if PINNED_FILES[p] != (digest, size):
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
    try:
        paths = exempt_paths(config)
    except ValueError as err:
        print("FAIL", err)
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
