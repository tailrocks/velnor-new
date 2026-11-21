#!/usr/bin/env bash
set -euo pipefail

root="${1:-$(git rev-parse --show-toplevel)}"
cd "$root"

locked=()
if [ -f Cargo.lock ]; then
  locked=(--locked)
fi

status=0
listing="$(cargo test --doc --workspace --all-features "${locked[@]}" -- --list 2>&1)" || status=$?

doctests="$(printf '%s\n' "$listing" | grep -E ': test$' || true)"
if [ -n "$doctests" ]; then
  printf '%s\n' "$doctests" >&2
  echo "no-doctests: rustdoc lists the doctests above. Doctests are off by decision (docs/proposed/rust-test-policy.md); remove the doc fence or indented code block." >&2
  exit 1
fi
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$listing" >&2
  echo "no-doctests: rustdoc doctest discovery failed (exit $status)." >&2
  exit "$status"
fi
echo "no-doctests: rustdoc lists no doctests"
