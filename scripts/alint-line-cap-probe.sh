#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
PROBE="$ROOT/ci-helper/crates/velnor-ci/src/alint_line_cap_probe.rs"
LOG="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/alint-line-cap-probe.log"

cleanup() {
  rm -f "$PROBE" "$LOG"
}
trap cleanup EXIT

cd "$ROOT" || exit 1

for lines in 401 400; do
  rm -f "$PROBE"
  seq 1 "$lines" | sed 's/.*/const L_&: u32 = &;/' > "$PROBE" || exit 1
  rc=0
  alint --format github --config .alint.yml --fail-on-warning check . > "$LOG" 2>&1 || rc=$?
  cat "$LOG"
  if [ "$lines" -eq 401 ]; then
    if [ "$rc" -eq 0 ] || ! grep -q 'alint_line_cap_probe.rs' "$LOG"; then
      echo "::error::alint accepted a 401-line Rust file" >&2
      exit 1
    fi
  elif [ "$rc" -ne 0 ]; then
    echo "::error::alint rejected a 400-line Rust file" >&2
    exit 1
  fi
done

rm -f "$PROBE"
