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
EXCLUDE = re.compile(r"^(\s*)exclude:\s*(.*?)\s*$")
ITEM = re.compile(r"^(\s*)-\s+(.*?)\s*$")
SCALAR = re.compile(r"""^(?:"([^"]+)"|'([^']+)'|([^\s#'",\[\]][^#]*?))\s*(?:#.*)?$""")
FLOW_SCALAR = re.compile(r"""\s*(?:"([^"]*)"|'([^']*)'|([^\s,'"\[\]][^,\]]*?))\s*(?:,|$)""")


def scalar(text):
    found = SCALAR.match(text)
    if not found:
        raise ValueError(f"unsupported exemption entry: {text}")
    return found.group(1) or found.group(2) or found.group(3)


def flow_items(body):
    items = []
    position = 0
    body = body.strip()
    while position < len(body):
        found = FLOW_SCALAR.match(body, position)
        if not found or found.end() == position:
            raise ValueError(f"unsupported exemption list: [{body}]")
        items.append(found.group(1) or found.group(2) or found.group(3))
        position = found.end()
    return items


def exempt_paths(config):
    paths = set()
    rule = None
    block = None
    for line in config.read_text().splitlines():
        if line.startswith("  - id: "):
            rule = line.removeprefix("  - id: ").strip()
            block = None
            continue
        if rule not in RULES:
            continue
        key = EXCLUDE.match(line)
        if key:
            block = None
            rest = re.sub(r"\s+#.*$", "", key.group(2))
            if rest.startswith("["):
                if not rest.endswith("]"):
                    raise ValueError(f"unsupported multi-line exemption list in {rule}")
                paths.update(flow_items(rest[1:-1]))
            elif rest:
                raise ValueError(f"unsupported exemption list in {rule}: {rest}")
            else:
                block = len(key.group(1))
            continue
        if block is None:
            continue
        item = ITEM.match(line)
        if item and len(item.group(1)) > block:
            paths.add(scalar(item.group(2)))
        elif line.strip() and not line.lstrip().startswith("#"):
            block = None
    paths.discard(".alint.yml")
    return paths


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(manifest):
    return dict(line.split("  ", 1)[::-1] for line in manifest.read_text().splitlines() if line)


def check(root, config, manifest):
    try:
        exempt = exempt_paths(config)
    except ValueError as error:
        return [str(error)]
    pinned = load(manifest)
    problems = [f"exempt but not pinned: {path}" for path in sorted(exempt - pinned.keys())]
    problems += [f"pinned but not exempt: {path}" for path in sorted(pinned.keys() - exempt)]
    for path in sorted(exempt & pinned.keys()):
        target = root / path
        if target.is_symlink():
            problems.append(f"symlink: {path}")
        elif not target.is_file():
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
