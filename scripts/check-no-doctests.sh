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

for flags in "${RUSTDOCFLAGS:-}" "${CARGO_ENCODED_RUSTDOCFLAGS:-}"; do
  case "$flags" in
    *--test-args*)
      echo "no-doctests: RUSTDOCFLAGS holds --test-args, which filters rustdoc's listing; unset it and run again." >&2
      exit 2
      ;;
  esac
done

dropped=()
while IFS='=' read -r name _; do
  case "$name" in
    CARGO_BUILD_RUSTDOCFLAGS | CARGO_TARGET_*_RUSTDOCFLAGS) dropped+=("$name") ;;
  esac
done < <(env)
dir="$PWD"
while :; do
  for config in "$dir/.cargo/config.toml" "$dir/.cargo/config"; do
    if [ -f "$config" ] && grep -q rustdocflags "$config"; then dropped+=("$config"); fi
  done
  [ "$dir" = / ] && break
  dir="$(dirname "$dir")"
done
for config in "${CARGO_HOME:-$HOME/.cargo}/config.toml" "${CARGO_HOME:-$HOME/.cargo}/config"; do
  if [ -f "$config" ] && grep -q rustdocflags "$config"; then dropped+=("$config"); fi
done
if [ "${#dropped[@]}" -gt 0 ]; then
  printf 'no-doctests: %s\n' "${dropped[@]}" >&2
  echo "no-doctests: the sources above set rustdocflags, and the RUSTDOCFLAGS this check sets replaces them, so rustdoc would list different doctests than cargo test --doc runs; remove them and run again." >&2
  exit 2
fi

encoded=()
if [ -n "${CARGO_ENCODED_RUSTDOCFLAGS+set}" ]; then
  separator=$'\x1f'
  value="${CARGO_ENCODED_RUSTDOCFLAGS:+$CARGO_ENCODED_RUSTDOCFLAGS$separator}--test${separator}--test-args${separator}--list"
  encoded=("CARGO_ENCODED_RUSTDOCFLAGS=$value")
fi

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
  listing="$(env RUSTDOCFLAGS="${RUSTDOCFLAGS:-} --test --test-args --list" ${encoded[@]+"${encoded[@]}"} cargo doc --workspace --no-deps --lib --bins --examples ${selection[@]+"${selection[@]}"} ${locked[@]+"${locked[@]}"} 2>"$diagnostics")" || status=$?
  if [ "$status" -ne 0 ]; then
    cat "$diagnostics" >&2
    printf '%s\n' "$listing" >&2
    echo "no-doctests: rustdoc doctest discovery failed (exit $status) with $features features." >&2
    exit "$status"
  fi
  parsed="$(printf '%s\n' "$listing" | awk '
    { line[NR] = $0 }
    /^[0-9]+ tests?, [0-9]+ benchmarks$/ {
      summaries++
      n = $1 + 0
      if (n == 0) next
      if (NR < n + 2 || line[NR - 1] != "") { bad = 1; next }
      for (i = NR - 1 - n; i < NR - 1; i++) print line[i]
    }
    END { if (bad || !summaries) exit 3 }
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
