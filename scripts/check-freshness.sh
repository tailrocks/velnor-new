#!/usr/bin/env bash
# Fail-closed freshness gate over the Velnor-owned version inventories.
#
# Validates four separate concerns:
#   local-pin          compiled-in constants == reviewed inventory pins,
#                      including the policy mirror and cargo-mutants pin.
#   effective-identity declared requirements == each Cargo.lock resolution,
#                      across discovered workspaces and all dependency scopes.
#   upstream-freshness reviewed pins have fresh upstream evidence; stale pins,
#                      stale evidence, and lookup failures fail closed.
#   advisories         deny policy and optional live scan cover every Cargo
#                      workspace, with no ignored advisories.
#
# Machine-readable output: every `row: {...}` line on stdout is one compact
# JSON object with keys check/subject/status/detail. `status` is pass, fail,
# or info. Every fail row makes this command exit nonzero.
#
# Usage: scripts/check-freshness.sh [--root DIR] [--check-upstream]
#                                   [--with-advisories]
#   --root DIR         validate a fixture tree instead of this repository.
#   --check-upstream   bounded read-only probe: latest stable release lookup
#                      with a 10 s timeout and 512 KiB encoded/decoded cap.
#                      Writes nothing; CI runs it weekly, outside build gates.
#   --with-advisories  run the live Cargo Deny scan for every workspace
#                      (180 s timeout each) plus the local deny-policy checks.
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

if [[ -z "$ROOT" ]]; then
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
elif [[ ! -d "$ROOT" ]]; then
  echo "check-freshness: root is not a directory: $ROOT" >&2
  exit 1
else
  ROOT="$(CDPATH= cd -- "$ROOT" && pwd -P)"
fi
INV="$ROOT/.velnor/freshness-inventory.json"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ ! -f "$INV" ]]; then
  echo "check-freshness: missing inventory: $INV" >&2
  exit 1
fi
if ! command -v mise >/dev/null 2>&1; then
  echo "check-freshness: mise is required (install the pinned repository toolchain first)" >&2
  exit 1
fi

cd "$PROJECT_ROOT"
env \
  VELNOR_INTERNAL_OP=repo-policy-v1 \
  VELNOR_REPO_POLICY_ACTION=freshness \
  VELNOR_REPO_POLICY_ROOT="$ROOT" \
  VELNOR_REPO_POLICY_CHECK_UPSTREAM="$CHECK_UPSTREAM" \
  VELNOR_REPO_POLICY_WITH_ADVISORIES="$WITH_ADVISORIES" \
  mise exec -- cargo run --quiet --locked -p velnor-actions-cli --bin velnor-actions
