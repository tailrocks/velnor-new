#!/bin/sh
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
#
# Maintains the SHA-256 table of the mise binary that generated workflows pin
# (`MISE_BINARIES` in crates/velnor-render/src/mise_digest.rs, issue #2158).
# The digests come from the official `SHASUMS256.txt` of the mise release
# (the raw `mise-v<version>-<os>-<arch>` executables), never from a build.
#
# Usage: scripts/mise-binary-digests.sh [VERSION] [--check|--write] [--sums FILE [--sig FILE]]
#   VERSION  the mise release; defaults to MISE_VERSION in crates/velnor-render/src/steps.rs
#   (none)   print the table row for VERSION
#   --check  exit 1 unless the checked-in table has exactly that row
#   --write  replace the checked-in table with that row (a bump of MISE_VERSION
#            still edits steps.rs by hand, and the pinned test literals follow)
#   --sums   read SHASUMS256.txt from FILE instead of the release download
#   --sig    the minisign signature of --sums (default: FILE.minisig); always required
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
exec python3 -I - "$root" "$@" <<'PY'
import re
import shutil
import subprocess
import sys
import tempfile
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

mode, version, sums, sig = "print", None, None, None
it = iter(args)
for arg in it:
    if arg in ("--check", "--write"):
        mode = arg[2:]
    elif arg == "--sums":
        sums = next(it, None) or sys.exit("--sums needs a file")
    elif arg == "--sig":
        sig = next(it, None) or sys.exit("--sig needs a file")
    elif arg.startswith("-"):
        sys.exit(f"unknown option {arg}")
    else:
        version = arg
if version is None:
    found = re.search(r'const MISE_VERSION: &str = "([^"]+)";', open(steps).read())
    version = found.group(1) if found else sys.exit("MISE_VERSION not found in steps.rs")
if not re.fullmatch(r"\d{4}\.\d{1,2}\.\d{1,3}", version):
    sys.exit(f"{version} is not a mise release version")

class SignatureError(Exception):
    pass


def read(path):
    with open(path, "rb") as handle:
        return handle.read()


def download(name):
    url = f"https://github.com/jdx/mise/releases/download/v{version}/{name}"
    with urllib.request.urlopen(url, timeout=60) as response:
        return response.read()


def minisign():
    try:
        pinned = subprocess.run(
            ["mise", "which", "minisign"],
            capture_output=True,
            text=True,
            timeout=60,
            cwd=root,
        )
    except (OSError, subprocess.SubprocessError):
        pinned = None
    if pinned is not None and pinned.returncode == 0 and pinned.stdout.strip():
        return pinned.stdout.strip()
    return shutil.which("minisign")


def verify(raw, signature):
    tool = minisign()
    if tool is None:
        raise SignatureError("minisign is not installed (mise install minisign)")
    with tempfile.TemporaryDirectory(prefix="mise-binary-digests.") as work:
        manifest, signed = f"{work}/SHASUMS256.txt", f"{work}/SHASUMS256.txt.minisig"
        for path, data in ((manifest, raw), (signed, signature)):
            with open(path, "wb") as handle:
                handle.write(data)
        result = subprocess.run(
            [tool, "-V", "-m", manifest, "-x", signed, "-p", f"{root}/scripts/mise-minisign.pub"],
            capture_output=True,
            text=True,
            timeout=60,
        )
        if result.returncode != 0:
            raise SignatureError(f"minisign exited {result.returncode}: {result.stderr.strip()}")


try:
    if sums:
        raw, signature = read(sums), read(sig or sums + ".minisig")
    else:
        raw, signature = download("SHASUMS256.txt"), download("SHASUMS256.txt.minisig")
    verify(raw, signature)
    text = raw.decode()
except (OSError, UnicodeDecodeError, SignatureError, subprocess.SubprocessError) as error:
    sys.exit(f"SHASUMS256.txt of mise {version}: signature check failed, nothing trusted: {error}")

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
