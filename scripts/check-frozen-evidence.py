#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import hashlib
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = Path(__file__).with_name("frozen-evidence.sha256")
RULES = ("no-absolute-local-paths", "no-local-temp-paths")
EXEMPT = re.compile(r'^        - "([^"]+)"$')


def exempt_paths(config):
    paths = set()
    rule = None
    for line in config.read_text().splitlines():
        if line.startswith("  - id: "):
            rule = line.removeprefix("  - id: ").strip()
        found = EXEMPT.match(line)
        if found and rule in RULES and found.group(1) != ".alint.yml":
            paths.add(found.group(1))
    return paths


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(manifest):
    return dict(line.split("  ", 1)[::-1] for line in manifest.read_text().splitlines() if line)


def check(root, config, manifest):
    exempt = exempt_paths(config)
    pinned = load(manifest)
    problems = [f"exempt but not pinned: {path}" for path in sorted(exempt - pinned.keys())]
    problems += [f"pinned but not exempt: {path}" for path in sorted(pinned.keys() - exempt)]
    for path in sorted(exempt & pinned.keys()):
        target = root / path
        if not target.is_file():
            problems.append(f"missing: {path}")
        elif digest(target) != pinned[path]:
            problems.append(f"changed: {path}")
    return problems


def write(root, config, manifest):
    lines = [f"{digest(root / path)}  {path}" for path in sorted(exempt_paths(config))]
    manifest.write_text("\n".join(lines) + "\n")


def main(argv):
    config = ROOT / ".alint.yml"
    if argv[1:] == ["--write"]:
        write(ROOT, config, MANIFEST)
        return 0
    problems = check(ROOT, config, MANIFEST)
    for problem in problems:
        print(problem)
    if problems:
        print(f"{len(problems)} frozen evidence problem(s): an exempt evidence file must stay byte-identical to scripts/{MANIFEST.name}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
