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
config="$(RUSTC_BOOTSTRAP=1 cargo -Zunstable-options config get --format json 2>/dev/null)" || {
  echo "no-doctests: cargo could not read its configuration." >&2
  exit 2
}
if [ "$(printf '%s' "$config" | jq '[.build.rustdocflags?, (.target // {} | .[]? | .rustdocflags?)] | map(select(. != null)) | length')" -gt 0 ]; then
  dropped+=("cargo configuration")
fi
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

metadata="$(cargo metadata --no-deps --format-version 1 ${locked[@]+"${locked[@]}"})"
pairs=()
while IFS= read -r pair; do
  pairs+=("$pair")
done < <(printf '%s' "$metadata" | jq -r '.packages[] | .name as $p | .features | keys[] | select(. != "default") | "\($p)/\(.)"')
if [ "${#pairs[@]}" -gt 6 ]; then
  echo "no-doctests: ${#pairs[@]} workspace features is too many to enumerate every combination (limit 6)." >&2
  exit 2
fi
bases=(--no-default-features)
if [ "$(printf '%s' "$metadata" | jq '[.packages[].features | has("default")] | any')" = true ]; then
  bases+=(default)
fi
passes=()
for base in "${bases[@]}"; do
  for ((mask = 0; mask < (1 << ${#pairs[@]}); mask++)); do
    subset=""
    for i in "${!pairs[@]}"; do
      if (( (mask >> i) & 1 )); then subset+="${subset:+,}${pairs[$i]}"; fi
    done
    passes+=("$base|$subset")
  done
done

doctests=""
for pass in "${passes[@]}"; do
  base="${pass%%|*}"
  subset="${pass#*|}"
  features="${base} ${subset:+--features $subset}"
  selection=()
  if [ "$base" != default ]; then selection+=("$base"); fi
  if [ -n "$subset" ]; then selection+=(--features "$subset"); fi
  status=0
  listing="$(env RUSTDOCFLAGS="${RUSTDOCFLAGS:-} --test --test-args --list" ${encoded[@]+"${encoded[@]}"} cargo doc --workspace --no-deps --lib --bins --examples ${selection[@]+"${selection[@]}"} ${locked[@]+"${locked[@]}"} 2>"$diagnostics")" || status=$?
  if [ "$status" -ne 0 ]; then
    cat "$diagnostics" >&2
    printf '%s\n' "$listing" >&2
    echo "no-doctests: rustdoc doctest discovery failed (exit $status) with ${features}features." >&2
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
    echo "no-doctests: could not read rustdoc's listing with ${features}features." >&2
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
