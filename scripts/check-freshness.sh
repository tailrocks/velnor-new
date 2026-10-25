#!/usr/bin/env bash
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
  ROOT="$(CDPATH='' cd -- "$ROOT" && pwd -P)"
fi
INV="$ROOT/.velnor/freshness-inventory.json"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ ! -f "$INV" ]]; then
  echo "check-freshness: missing inventory: $INV" >&2
  exit 1
fi
if [[ -n "${VELNOR_ACTIONS_BIN:-}" && -x "$VELNOR_ACTIONS_BIN" ]]; then
  exec env \
    VELNOR_INTERNAL_OP=repo-policy-v1 \
    VELNOR_REPO_POLICY_ACTION=freshness \
    VELNOR_REPO_POLICY_ROOT="$ROOT" \
    VELNOR_REPO_POLICY_CHECK_UPSTREAM="$CHECK_UPSTREAM" \
    VELNOR_REPO_POLICY_WITH_ADVISORIES="$WITH_ADVISORIES" \
    "$VELNOR_ACTIONS_BIN"
fi

if ! command -v mise >/dev/null 2>&1; then
  echo "check-freshness: mise is required (install the pinned repository toolchain first)" >&2
  exit 1
fi

cd "$PROJECT_ROOT"
RUST_POLICY="$(awk -v key=rust -f "$PROJECT_ROOT/scripts/toml-tool-pin.awk" \
  "$ROOT/.velnor/version-policy.toml")" || {
  echo "check-freshness: version policy has no exact Rust pin" >&2
  exit 1
}
MBX_POLICY="$(awk -v key=mr-boxington -f "$PROJECT_ROOT/scripts/toml-tool-pin.awk" \
  "$ROOT/.velnor/version-policy.toml")" || {
  echo "check-freshness: version policy has no exact MBX pin" >&2
  exit 1
}
env \
  VELNOR_INTERNAL_OP=repo-policy-v1 \
  VELNOR_REPO_POLICY_ACTION=freshness \
  VELNOR_REPO_POLICY_ROOT="$ROOT" \
  VELNOR_REPO_POLICY_CHECK_UPSTREAM="$CHECK_UPSTREAM" \
  VELNOR_REPO_POLICY_WITH_ADVISORIES="$WITH_ADVISORIES" \
  mise exec "rust@$RUST_POLICY" "mr-boxington@$MBX_POLICY" -- cargo run --quiet --locked -p velnor-actions-cli --bin velnor-actions
