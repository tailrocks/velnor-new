#!/usr/bin/env bash
# Fail-closed freshness gate over the Velnor-owned version inventories.
#
# Validates four separated concerns (never conflated):
#   local-pin          compiled-in constants == reviewed inventory pins,
#                      including the version-policy mirror and the
#                      cargo-mutants activation pin.
#   effective-identity declared manifest requirements == Cargo.lock
#                      resolution, keyed by name+version+source with every
#                      dependency form and scope covered.
#   upstream-freshness reviewed pins are backed by fresh upstream evidence
#                      (source URL + check timestamp); stale evidence, stale
#                      pins, and lookup failures fail, never report current.
#   advisories         deny policy forbids ignored advisories; the live
#                      `cargo deny` scan runs in CI (or `--with-advisories`).
#
# Machine-readable output: every `row: {...}` line on stdout is one compact
# JSON object with keys check/subject/status/detail. `status` is one of
# pass/fail/info. Every fail row contributes to a nonzero exit; human `ok:`
# lines and the final PASS/FAIL summary are for logs only.
#
# Usage: scripts/check-freshness.sh [--root DIR] [--check-upstream]
#                                   [--with-advisories]
#   --root DIR         validate a fixture tree instead of this repository.
#   --check-upstream   bounded read-only upstream probe: refetch each row's
#                      latest stable release (10 s timeout and independent
#                      512 KiB encoded/decompressed caps per request) and
#                      fail stale pins and lookup failures.
#                      Writes nothing; run by the generated weekly
#                      `.github/workflows/freshness.yml`, never gating builds.
#   --with-advisories  run the live `cargo deny check advisories` scan
#                      (180 s timeout) in addition to the deny-policy checks.
set -euo pipefail

ROOT=""
CHECK_UPSTREAM=0
WITH_ADVISORIES=0

usage() {
  echo "usage: scripts/check-freshness.sh [--root DIR] [--check-upstream] [--with-advisories]"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)
      if [[ $# -lt 2 ]]; then
        echo "check-freshness: --root needs a directory" >&2
        exit 2
      fi
      ROOT="$2"
      shift 2
      ;;
    --check-upstream)
      CHECK_UPSTREAM=1
      shift
      ;;
    --with-advisories)
      WITH_ADVISORIES=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "check-freshness: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -z "$ROOT" ]]; then
  ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
fi
INV="$ROOT/.velnor/freshness-inventory.json"

if [[ ! -f "$INV" ]]; then
  echo "check-freshness: missing inventory: $INV" >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "check-freshness: python3 is required" >&2
  exit 1
fi

PYTHONDONTWRITEBYTECODE=1 python3 "$SCRIPT_DIR/freshness_checks/main.py" \
  "$ROOT" "$INV" "$CHECK_UPSTREAM" "$WITH_ADVISORIES"
