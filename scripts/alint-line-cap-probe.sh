#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
PROBE="$ROOT/crates/velnor-ci/src/alint_line_cap_probe.rs"
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
python3 -I scripts/check-digest-frozen-evidence.py "$ROOT" "$ROOT/.alint.yml" || exit 1

attack_case() {
  local label="$1" exclude="$2" attack out rc
  attack="$(mktemp -d "${TMPDIR:-/tmp}/digest-attack.XXXXXX")" || exit 1
  mkdir -p "$attack/docs/reviews" || exit 1
  printf 'ordinary doc\n' > "$attack/docs/attack.md"
  sha="$(shasum -a 256 "$attack/docs/attack.md" | cut -d' ' -f1)"
  printf '{"base_checkpoint": {"manifest": "docs/attack.md", "manifest_sha256": "%s"}}\n' "$sha" \
    > "$attack/docs/reviews/phasec-registered-source-fixes-v32.json"
  printf 'version: 1\nrules:\n  - id: no-local-temp-paths\n    paths:\n%b\n' "$exclude" \
    > "$attack/.alint.yml"
  out="$(python3 -I scripts/check-digest-frozen-evidence.py "$attack" "$attack/.alint.yml" 2>&1)"
  rc=$?
  rm -rf "$attack"
  if [ "$rc" -eq 0 ] || ! grep -qE 'docs/attack.md: sha256 not recorded|unsupported' <<<"$out"; then
    echo "::error::digest guard accepted an exempt doc ($label) recorded only by an unpinned claim file" >&2
    exit 1
  fi
  echo "digest guard negative control: OK ($label rejected)"
}
attack_case double-quoted '      exclude:\n        - "docs/attack.md"'
attack_case single-quoted "      exclude:\n        - 'docs/attack.md'"
attack_case unquoted '      exclude:\n        - docs/attack.md'
attack_case unquoted-comment '      exclude:\n        - docs/attack.md # why'
attack_case shallow-indent '      exclude:\n      - docs/attack.md'
attack_case flow-list '      exclude: [docs/attack.md]'

rm -f "$PROBE"
