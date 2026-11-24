#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -uo pipefail

if [ -z "${ALINT_PROBE_PINNED_PYTHON:-}" ]; then
  ALINT_PROBE_PINNED_PYTHON=1 exec mise exec python -- bash "${BASH_SOURCE[0]}" "$@"
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
PROBE="$ROOT/crates/velnor-ci/src/alint_line_cap_probe.rs"
LOG="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/alint-line-cap-probe.log"

cleanup() {
  rm -f "$PROBE" "$LOG"
}
trap cleanup EXIT

cd "$ROOT" || exit 1

python3 -m unittest scripts/test_private_names_guard.py scripts/test_private_names_spellings.py scripts/test_frozen_evidence.py || exit 1
bash scripts/test-run-private-names-guard.sh || exit 1
python3 scripts/check-frozen-evidence.py || exit 1
bash scripts/run-private-names-guard.sh || exit 1

for lines in 401 400; do
  rm -f "$PROBE"
  seq 1 "$lines" | sed 's/.*/const L_&: u32 = &;/' > "$PROBE" || exit 1
  rc=0
  alint --format github --config .alint.yml --fail-on-warning check . > "$LOG" 2>&1 || rc=$?
  sed 's/^::/probe output: /' "$LOG"
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

bash scripts/test-no-doctests-rule.sh || exit 1
bash scripts/test-no-absolute-local-paths-rule.sh || exit 1

rm -f "$PROBE"
