#!/usr/bin/env bash
set -euo pipefail

tar_bin="${TAR_BIN:-tar}"
if ! "$tar_bin" --version | sed -n '1p' | grep -q '^tar (GNU tar) '; then
  echo "GNU tar is required." >&2
  exit 1
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/velnor-gnu-tar-extended.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
export work tar_bin
passed=0
failed=0

run_case() {
  local name="$1" test_function="$2" function_source
  function_source="$(declare -f)"
  if bash -euo pipefail -c "$function_source"$'\n'"$test_function"; then
    passed=$((passed + 1))
    printf 'PASS %s\n' "$name"
  else
    failed=$((failed + 1))
    printf 'FAIL %s\n' "$name" >&2
  fi
}

case_cli_forms_and_dash_member() {
  local src="$work/cli-src" dest="$work/cli-dest"
  mkdir -p -- "$src" "$dest"
  printf 'dash-ok\n' > "$src/-dash-member"
  "$tar_bin" cf "$work/old.tar" -C "$src" -- -dash-member
  "$tar_bin" -xf "$work/old.tar" -C "$dest"
  grep -qx 'dash-ok' "$dest/-dash-member"
  "$tar_bin" cfz "$work/clustered.tar.gz" -C "$src" -- -dash-member
  rm -f -- "$dest/-dash-member"
  "$tar_bin" xfz "$work/clustered.tar.gz" -C "$dest"
  grep -qx 'dash-ok' "$dest/-dash-member"
}

case_manifest_over_two_mib() {
  local src="$work/large-src" dest="$work/large-dest"
  local leaf name i bytes count selected
  mkdir -p -- "$src" "$dest"
  leaf=""
  for ((i = 0; i < 198; i++)); do leaf+="a"; done
  : > "$work/manifest"
  for ((i = 1; i <= 10500; i++)); do
    name="$(printf '%s-%05d' "$leaf" "$i")"
    : > "$src/$name"
    printf '%s\n' "$name" >> "$work/manifest"
  done
  printf 'short-ok\n' > "$src/short-ok"
  printf 'short-ok\n' >> "$work/manifest"
  bytes="$(wc -c < "$work/manifest" | tr -d ' ')"
  [ "$bytes" -gt 2097152 ]
  "$tar_bin" -cf "$work/large.tar" -C "$src" --files-from "$work/manifest"
  count="$("$tar_bin" -tf "$work/large.tar" | wc -l | tr -d ' ')"
  [ "$count" = 10501 ]
  printf 'short-ok\n' > "$work/one-member"
  "$tar_bin" -xf "$work/large.tar" -C "$dest" --files-from "$work/one-member"
  grep -qx 'short-ok' "$dest/short-ok"
  selected="$(find "$dest" -type f | wc -l | tr -d ' ')"
  [ "$selected" = 1 ]
}

run_case legacy-cli-and-dash-member case_cli_forms_and_dash_member
run_case files-from-manifest-over-two-mib case_manifest_over_two_mib
printf 'RESULT pass=%s fail=%s\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
