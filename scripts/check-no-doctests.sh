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

# An inherited `--test-args` would filter rustdoc's listing and hide doctests.
for flags in "${RUSTDOCFLAGS:-}" "${CARGO_ENCODED_RUSTDOCFLAGS:-}"; do
  case "$flags" in
    *--test-args*)
      echo "no-doctests: RUSTDOCFLAGS holds --test-args, which filters rustdoc's listing; unset it and run again." >&2
      exit 2
      ;;
  esac
done

passes=(--all-features --no-default-features)
metadata="$(cargo metadata --no-deps --format-version 1 ${locked[@]+"${locked[@]}"})"
if printf '%s' "$metadata" | grep -Eq '"features":\{([^{}]*,)?"default":'; then
  passes+=(default)
fi

doctests=""
for features in "${passes[@]}"; do
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
  # rustdoc ends each listing group with "N tests, M benchmarks" after a blank line.
  # Only the N lines above that blank line are doctests; other stdout, such as a
  # procedural macro that prints a doctest-shaped line, is not.
  parsed="$(printf '%s\n' "$listing" | awk '
    { line[NR] = $0 }
    /^[0-9]+ tests?, [0-9]+ benchmarks$/ {
      n = $1 + 0
      if (n == 0) next
      if (NR < n + 2 || line[NR - 1] != "") { bad = 1; next }
      for (i = NR - 1 - n; i < NR - 1; i++) print line[i]
    }
    END { if (bad) exit 3 }
  ')" || {
    printf '%s\n' "$listing" >&2
    echo "no-doctests: could not read rustdoc's listing with $features features." >&2
    exit 3
  }
  doctests+="$parsed"$'\n'
done

doctests="$(printf '%s' "$doctests" | grep . | sort -u || true)"
if [ -n "$doctests" ]; then
  printf '%s\n' "$doctests" >&2
  echo "no-doctests: rustdoc lists the doctests above. Doctests are off by decision (docs/proposed/rust-test-policy.md); remove the doc fence or indented code block." >&2
  exit 1
fi
echo "no-doctests: rustdoc lists no doctests (${#passes[@]} feature passes)"
