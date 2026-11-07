#!/bin/sh
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -eu

case "$(uname -s)" in
  Darwin) ;;
  *) printf '%s\n' 'this fixture requires Darwin' >&2; exit 1 ;;
esac

output_dir=${1:?private output directory required}
case "$output_dir" in
  /*) ;;
  *) printf '%s\n' 'output directory must be absolute' >&2; exit 1 ;;
esac

script_dir=$(CDPATH= cd -P -- "$(dirname -- "$0")/.." && pwd -P)
source_path="$script_dir/crates/velnor-actions-mise/tests/fixtures/group_signal_zero_interpose.c"
compiler=/usr/bin/cc
library="$output_dir/group_signal_zero_interpose.dylib"
receipt="$output_dir/group_signal_zero_interpose.json"
umask 077
mkdir -m 700 -- "$output_dir"
output_dir=$(CDPATH= cd -P -- "$output_dir" && pwd -P)
library="$output_dir/group_signal_zero_interpose.dylib"
receipt="$output_dir/group_signal_zero_interpose.json"
"$compiler" -dynamiclib -o "$library" "$source_path"
chmod 500 "$library"
compiler_version=$("$compiler" --version 2>&1 | sed -n '1p')
/usr/bin/python3 - "$output_dir" "$library" "$source_path" "$compiler" "$compiler_version" <<'PY'
import hashlib
import json
import os
import pathlib
import sys

output_dir, library, source, compiler, compiler_version = sys.argv[1:]
source_path = pathlib.Path(source).resolve(strict=True)
library_path = pathlib.Path(library).resolve(strict=True)
compiler_path = pathlib.Path(compiler).resolve(strict=True)
receipt = {
    "library_path": str(library_path),
    "library_sha256": hashlib.sha256(library_path.read_bytes()).hexdigest(),
    "source_path": str(source_path),
    "source_sha256": hashlib.sha256(source_path.read_bytes()).hexdigest(),
    "compiler_path": str(compiler_path),
    "compiler_version": compiler_version,
    "compile_argv": [compiler, "-dynamiclib", "-o", str(library_path), str(source_path)],
}
receipt_path = pathlib.Path(output_dir) / "group_signal_zero_interpose.json"
with receipt_path.open("x", encoding="utf-8") as stream:
    json.dump(receipt, stream, separators=(",", ":"), sort_keys=True)
    stream.write("\n")
os.chmod(receipt_path, 0o400)
PY
