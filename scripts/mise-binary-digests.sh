#!/bin/sh
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
#
# Maintains the SHA-256 table of the mise binary that generated workflows pin
# (`MISE_BINARIES` in crates/velnor-render/src/mise_digest.rs, issue #2158).
# The digests come from the official `SHASUMS256.txt` of the mise release
# (the raw `mise-v<version>-<os>-<arch>` executables), never from a build.
#
# Usage: scripts/mise-binary-digests.sh [VERSION] [--check|--write] [--sums FILE]
#   VERSION  the mise release; defaults to MISE_VERSION in crates/velnor-render/src/steps.rs
#   (none)   print the table row for VERSION
#   --check  exit 1 unless the checked-in table has exactly that row
#   --write  replace the checked-in table with that row (a bump of MISE_VERSION
#            still edits steps.rs by hand, and the pinned test literals follow)
#   --sums   read SHASUMS256.txt from FILE instead of the release download
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
exec python3 -I - "$root" "$@" <<'PY'
import re
import sys
import urllib.request

root, args = sys.argv[1], sys.argv[2:]
steps = f"{root}/crates/velnor-render/src/steps.rs"
table = f"{root}/crates/velnor-render/src/mise_digest.rs"
platforms = {
    "linux-arm64": "Linux-ARM64",
    "linux-x64": "Linux-X64",
    "macos-arm64": "macOS-ARM64",
    "macos-x64": "macOS-X64",
}

mode, version, sums = "print", None, None
it = iter(args)
for arg in it:
    if arg in ("--check", "--write"):
        mode = arg[2:]
    elif arg == "--sums":
        sums = next(it, None) or sys.exit("--sums needs a file")
    elif arg.startswith("-"):
        sys.exit(f"unknown option {arg}")
    else:
        version = arg
if version is None:
    found = re.search(r'const MISE_VERSION: &str = "([^"]+)";', open(steps).read())
    version = found.group(1) if found else sys.exit("MISE_VERSION not found in steps.rs")
if not re.fullmatch(r"\d{4}\.\d{1,2}\.\d{1,3}", version):
    sys.exit(f"{version} is not a mise release version")

if sums:
    text = open(sums).read()
else:
    url = f"https://github.com/jdx/mise/releases/download/v{version}/SHASUMS256.txt"
    with urllib.request.urlopen(url, timeout=60) as response:
        text = response.read().decode()

digests = {}
for line in text.splitlines():
    parts = line.split()
    if len(parts) != 2:
        continue
    digest, name = parts[0], parts[1].removeprefix("./")
    for suffix, key in platforms.items():
        if name == f"mise-v{version}-{suffix}":
            if not re.fullmatch(r"[0-9a-f]{64}", digest):
                sys.exit(f"{name}: {digest} is not a lower-case SHA-256")
            digests[key] = digest
missing = sorted(set(platforms.values()) - set(digests))
if missing:
    sys.exit(f"SHASUMS256.txt of mise {version} has no raw executable for: {', '.join(missing)}")

rows = "".join(
    f'        (\n            "{key}",\n            "{digests[key]}",\n        ),\n'
    for key in sorted(digests)
)
row = (
    "pub const MISE_BINARIES: &[MiseBinary] = &[MiseBinary {\n"
    f'    version: "{version}",\n    sha256: [\n{rows}    ],\n}}];\n'
)
current = open(table).read()
block = re.search(r"pub const MISE_BINARIES: &\[MiseBinary\] = &\[.*?\n\}\];\n", current, re.S)
if block is None:
    sys.exit("MISE_BINARIES not found in mise_digest.rs")
if mode == "print":
    sys.stdout.write(row)
elif mode == "check":
    if block.group(0) != row:
        sys.exit(f"MISE_BINARIES differs from the official SHASUMS256.txt of mise {version}:\n{row}")
    print(f"MISE_BINARIES matches the official SHASUMS256.txt of mise {version}")
else:
    open(table, "w").write(current[: block.start()] + row + current[block.end():])
    print(f"wrote MISE_BINARIES for mise {version}")
PY
