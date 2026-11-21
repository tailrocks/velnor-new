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
  printf 'version: 1\nrules:\n  - id: no-absolute-local-paths\n    kind: file_content_forbidden\n    level: error\n    paths:\n      include: ["**/*.md"]\n    pattern: "x"\n  - id: no-local-temp-paths\n    kind: file_content_forbidden\n    level: error\n    paths:\n      include: ["**/*.md"]\n%b\n    pattern: "x"\n' "$exclude" \
    > "$attack/.alint.yml"
  out="$(python3 -I scripts/check-digest-frozen-evidence.py "$attack" "$attack/.alint.yml" 2>&1)"
  rc=$?
  rm -rf "$attack"
  if [ "$rc" -eq 0 ] || ! grep -qE 'docs/attack.md: sha256 not recorded|unsupported|refusing|could not resolve|unexpected line' <<<"$out"; then
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
attack_case non-list-key-between-entries '      exclude:\n        - .alint.yml\n        note: stray\n        - docs/attack.md'

manifest_case() {
  local attack out rc ev=docs/implemented/evidence/freshness-2026-10-24
  attack="$(mktemp -d "${TMPDIR:-/tmp}/digest-attack.XXXXXX")" || exit 1
  mkdir -p "$attack/$ev" || exit 1
  printf 'rewritten receipt\n' > "$attack/$ev/receipt.txt"
  printf '%s  receipt.txt\n' "$(shasum -a 256 "$attack/$ev/receipt.txt" | cut -d' ' -f1)" > "$attack/$ev/SHA256SUMS"
  printf 'version: 1\nrules:\n  - id: no-absolute-local-paths\n    kind: file_content_forbidden\n    level: error\n    paths:\n      include: ["**/*.md"]\n    pattern: "x"\n  - id: no-local-temp-paths\n    kind: file_content_forbidden\n    level: error\n    paths:\n      include: ["**/*.md"]\n      exclude:\n        - %s/receipt.txt\n    pattern: "x"\n' "$ev" \
    > "$attack/.alint.yml"
  out="$(python3 -I scripts/check-digest-frozen-evidence.py "$attack" "$attack/.alint.yml" 2>&1)"
  rc=$?
  rm -rf "$attack"
  if [ "$rc" -eq 0 ] || ! grep -q 'manifest differs from its pinned sha256' <<<"$out"; then
    echo "::error::digest guard accepted a receipt rewritten together with its manifest" >&2
    exit 1
  fi
  echo "digest guard negative control: OK (rewritten manifest rejected)"
}
manifest_case

# One path exempt from both rules is verified once and accepted; a duplicate inside one rule is refused.
dup_case() {
  local label="$1" checker="$2" cfg out rc
  cfg="$(mktemp "${TMPDIR:-/tmp}/digest-dup.XXXXXX")" || exit 1
  awk -v dup="$3" '{print} /^  - id: no-absolute-local-paths/{r=1} r && /^      exclude:/{print dup; r=0}' .alint.yml > "$cfg"
  out="$(python3 -I "$checker" "$ROOT" "$cfg" 2>&1)"
  rc=$?
  rm -f "$cfg"
  echo "$out" | sed "s/^/digest guard ($label): /"
  if [ "$4" = accept ] && { [ "$rc" -ne 0 ] || ! grep -q 'digest-frozen evidence check: OK' <<<"$out"; }; then
    echo "::error::digest guard refused a path exempt from two rules" >&2
    exit 1
  fi
  if [ "$4" = refuse ] && { [ "$rc" -eq 0 ] || ! grep -q 'duplicate exclude entries' <<<"$out"; }; then
    echo "::error::digest guard accepted a duplicate exclude inside one rule" >&2
    exit 1
  fi
}
dup_case cross-rule scripts/check-digest-frozen-evidence.py '        - docs/reviews/phasec-registered-source-fixes-v31.json' accept
dup_case same-rule scripts/check-digest-frozen-evidence.py '        - .alint.yml' refuse

rm -f "$PROBE"
