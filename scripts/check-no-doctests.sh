#!/usr/bin/env bash
set -euo pipefail

root="${1:-$(git rev-parse --show-toplevel)}"
cd "$root"

locked=()
if [ -f Cargo.lock ]; then
  locked=(--locked)
fi

diagnostics="$(mktemp)"
trap 'rm -f "$diagnostics"' EXIT

doctests=""
for features in --all-features --no-default-features default; do
  selection=()
  if [ "$features" != default ]; then
    selection=("$features")
  fi
  status=0
  listing="$(RUSTDOCFLAGS="${RUSTDOCFLAGS:-} --test --test-args --list" cargo doc --workspace --no-deps --lib --bins --examples ${selection[@]+"${selection[@]}"} ${locked[@]+"${locked[@]}"} 2>"$diagnostics")" || status=$?
  if [ "$status" -ne 0 ]; then
    cat "$diagnostics" >&2
    printf '%s\n' "$listing" >&2
    echo "no-doctests: rustdoc doctest discovery failed (exit $status) with $features features." >&2
    exit "$status"
  fi
  doctests+="$(printf '%s\n' "$listing" | grep -E ' \(line [0-9]+\): test$' || true)"$'\n'
done

doctests="$(printf '%s' "$doctests" | grep . | sort -u || true)"
if [ -n "$doctests" ]; then
  printf '%s\n' "$doctests" >&2
  echo "no-doctests: rustdoc lists the doctests above. Doctests are off by decision (docs/proposed/rust-test-policy.md); remove the doc fence or indented code block." >&2
  exit 1
fi
echo "no-doctests: rustdoc lists no doctests"
