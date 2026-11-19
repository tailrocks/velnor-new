#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Second stage of `stable-sources.sh resolve-tools`. The body below is the text of the engine's
# "Resolve project tool versions" step, unchanged but for its quoting: the engine passed it to
# `bash -c` inside single quotes, which this file does not need. It runs in the scrubbed shell that
# `stable-sources.sh` starts, so nothing here reads a credential.

unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN MISE_GITHUB_TOKEN GH_TOKEN GH_HOST GH_CONFIG_DIR; set -eu
: "${GITHUB_ENV:?missing GITHUB_ENV}"
: "${RUNNER_TEMP:?missing RUNNER_TEMP}"
velnor_dir="$RUNNER_TEMP/velnor-project-tool"
velnor_db="$velnor_dir/declared"
velnor_tab=$(printf '\t')
velnor_safe() { printf '%s' "$1" | cut -c1-64 | tr -c 'A-Za-z0-9._/@+-' '?'; }
velnor_fail() { printf 'velnor: %s\n' "$1" >&2; exit 1; }
velnor_capture_physical_path() {
  [ "$#" -eq 2 ] || return 1
  (cd -P "$1" && pwd -P) > "$2" || return 1
  resolved_path=
  newline='
'
  while IFS= read -r path_line; do
    resolved_path=$resolved_path$path_line"$newline"
  done < "$2"
  case "$resolved_path" in
    *"$newline") resolved_path=${resolved_path%"$newline"} ;;
    *) return 1 ;;
  esac
  printf '%s\n' "$resolved_path" > "$2" || return 1
}; velnor_bind_selected_mise() { [ "$#" -eq 1 ] || return 5; mise_data_dir=${MISE_DATA_DIR-}; if [ -z "$mise_data_dir" ]; then if [ -n "${XDG_DATA_HOME-}" ]; then mise_data_dir="$XDG_DATA_HOME/mise"; elif [ -n "${HOME-}" ]; then mise_data_dir="$HOME/.local/share/mise"; else return 1; fi; fi; case "$mise_data_dir" in /*) ;; *) mise_data_dir=./$mise_data_dir;; esac; case "$mise_data_dir" in /) ;; */) mise_data_dir=${mise_data_dir%/};; esac; [ -d "$mise_data_dir" ] || return 2; velnor_capture_physical_path "$mise_data_dir" "$1" || return 3; resolved_mise_data_dir=$resolved_path; [ -d "$resolved_mise_data_dir/bin" ] || return 4; velnor_capture_physical_path "$resolved_mise_data_dir/bin" "$1" || return 4; resolved_mise_bin_dir=$resolved_path; mise_bin="$resolved_mise_bin_dir/mise"; [ -f "$mise_bin" ] && [ -x "$mise_bin" ] && [ ! -L "$mise_bin" ] || return 4; MISE_DATA_DIR="$resolved_mise_data_dir"; export MISE_DATA_DIR; return 0; }
velnor_mise() { "$mise_bin" --no-config --no-env --no-hooks "$@"; }
rm -rf "$velnor_dir"
mkdir -p "$velnor_dir"
: > "$velnor_db"
velnor_bind_selected_mise "$velnor_dir/mise-data-dir" || { velnor_mise_status=$?; velnor_fail "Setup Mise data directory or binary is invalid (status $velnor_mise_status)"; }
velnor_regular() {
  if [ -L "$1" ] || [ ! -f "$1" ]; then velnor_fail "$1 must be a regular file"; fi
}
velnor_toml() {
  cp -- "$1" "$velnor_dir/src.toml"
  velnor_mise config get -f "$velnor_dir/src.toml" > /dev/null 2>&1 || velnor_fail "$1 is not valid TOML"
}
velnor_tools() {
  velnor_toml "$1"
  if velnor_mise config get -f "$velnor_dir/src.toml" tools > "$velnor_dir/tools" 2>/dev/null; then
    awk -v file="$1" '
function unq(s) {
  if (s ~ /^"[^"\\]*"$/ || s ~ /^\047[^\047]*\047$/) { return substr(s, 2, length(s) - 2) }
  return s
}
BEGIN { OFS = "\t"; cur = ""; multi = "" }
{
  line = $0
  if (multi != "") { if (index(line, multi) > 0) { multi = "" } next }
  if (line ~ /^[ \t]*(#|$)/) { next }
  if (line ~ /^[ \t]*\[/) {
    hdr = line
    sub(/^[ \t]*\[+[ \t]*/, "", hdr)
    sub(/[ \t]*\]+[ \t]*$/, "", hdr)
    cur = unq(hdr)
    if (cur == "") { cur = "?" }
    next
  }
  eq = index(line, " = ")
  if (eq == 0) { next }
  k = unq(substr(line, 1, eq - 1))
  v = substr(line, eq + 3)
  if (v ~ /^"""/ || v ~ /^\047\047\047/) {
    d = substr(v, 1, 3)
    if (index(substr(v, 4), d) == 0) { multi = d }
    v = d
  } else {
    v = unq(v)
  }
  if (cur == "") { print file, k, v } else if (k == "version") { print file, cur, v }
}' "$velnor_dir/tools" >> "$velnor_db"
  fi
}
velnor_tool_versions() {
  awk -v file="$1" '
BEGIN { OFS = "\t" }
{
  sub(/[ \t]*#.*$/, "")
  if ($0 ~ /^[ \t]*$/) { next }
  line = $0
  sub(/^[ \t]+/, "", line)
  k = line
  sub(/[ \t].*$/, "", k)
  v = line
  if (v == k) { v = "" } else { sub(/^[^ \t]+[ \t]+/, "", v); sub(/[ \t]+$/, "", v) }
  print file, k, v
}' "$1" >> "$velnor_db"
}
velnor_toolchain() {
  if grep -q '^[[:space:]]*\[' "$1"; then
    velnor_toml "$1"
    if velnor_mise config get -f "$velnor_dir/src.toml" toolchain.channel > "$velnor_dir/channel" 2>/dev/null; then
      awk -v file="$1" 'BEGIN { OFS = "\t" } NR == 1 { v = $0; next } { v = v " " $0 } END { print file, "rust", v }' "$velnor_dir/channel" >> "$velnor_db"
    fi
  else
    printf '%s\t%s\t%s\n' "$1" rust "$(tr -d ' \t\r\n' < "$1")" >> "$velnor_db"
  fi
}
velnor_exact() {
  case "$2" in
    ''|*[!0-9.]*|.*|*.|*..*|*.*.*.*|0[0-9]*|*.0[0-9]*) velnor_fail "$1 must declare one exact X.Y.Z version, not '$(velnor_safe "$2")'" ;;
    *.*.*) ;;
    *) velnor_fail "$1 must declare one exact X.Y.Z version, not '$(velnor_safe "$2")'" ;;
  esac
}
velnor_tool() {
  name=$1
  shift
  resolved=
  origin=
  while IFS="$velnor_tab" read -r file key value; do
    matched=0
    for want in "$@"; do
      if [ "$key" = "$want" ]; then matched=1; fi
    done
    if [ "$matched" = 0 ]; then continue; fi
    velnor_exact "$file $key" "$value"
    if [ -n "$resolved" ] && [ "$resolved" != "$value" ]; then
      velnor_fail "$name: $file $key declares $value, $origin declares $resolved"
    fi
    if [ -z "$resolved" ]; then origin="$file $key"; fi
    resolved=$value
  done < "$velnor_db"
  [ -n "$resolved" ] || velnor_fail "$name: the project config no longer declares this tool; regenerate the workflow"
  printf '%s=%s\n' "$name" "$resolved" >> "$GITHUB_ENV"
  if [ "$name" = VELNOR_TOOL_RUST ] && [ "${velnor_export_rustup-0}" = 1 ]; then
    printf 'RUSTUP_TOOLCHAIN=%s\n' "$resolved" >> "$GITHUB_ENV"
  fi
  printf 'velnor: %s=%s\n' "$name" "$resolved"
}
# shellcheck disable=SC2016 # awk program text: awk, not the shell, expands it
velnor_lock_awk='BEGIN { q = "\""; e1 = "[[tools." key "]]"; e2 = "[[tools." q key q "]]"; e3 = "[tools." key "]"; e4 = "[tools." q key q "]"; s1 = "[tools." key "."; s2 = "[tools." q key q "."; p1 = s1 q "platforms." platform q "]"; p2 = s2 q "platforms." platform q "]"; p3 = s1 "platforms." platform "]"; p4 = s2 "platforms." platform "]"; want = "version = " q version q; inline = q "platforms." platform q " = {"; state = 0 }
{ line = $0; sub(/^[ \t]+/, "", line); sub(/[ \t\r]+$/, "", line) }
line == "" || substr(line, 1, 1) == "#" { next }
substr(line, 1, 1) == "[" {
  matched = state == 2 || state == 3 || state == 4; other = state == 1 || state == 5
  if (line == e1 || line == e2 || line == e3 || line == e4) state = 1
  else if (line == p1 || line == p2 || line == p3 || line == p4) state = matched ? 3 : (other ? 5 : 0)
  else if (index(line, s1) == 1 || index(line, s2) == 1) state = matched ? 4 : (other ? 5 : 0)
  else state = 0
  next
}
state == 1 && line == want { state = 2; next }
state == 3 && (index(line, "checksum = ") == 1 || index(line, "url = ") == 1) { print lock " " key " " line; next }
state == 2 && index(line, inline) == 1 {
  if (match(line, /checksum = "[^"]*"/)) print lock " " key " " substr(line, RSTART, RLENGTH)
  if (match(line, /[{ ,]url = "[^"]*"/)) print lock " " key " " substr(line, RSTART + 1, RLENGTH - 1)
}'
velnor_lock() {
  name=$1
  shift
  case "${RUNNER_OS-}:${RUNNER_ARCH-}" in
    Linux:X64) platform=linux-x64 ;;
    Linux:ARM64) platform=linux-arm64 ;;
    macOS:X64) platform=macos-x64 ;;
    macOS:ARM64) platform=macos-arm64 ;;
    Windows:X64) platform=windows-x64 ;;
    Windows:ARM64) platform=windows-arm64 ;;
    *) velnor_fail "$name: unsupported runner platform '${RUNNER_OS-}:${RUNNER_ARCH-}'" ;;
  esac
  : > "$velnor_dir/lock"
  for lock in $velnor_locks; do
    if [ ! -e "$lock" ] && [ ! -L "$lock" ]; then continue; fi
    velnor_regular "$lock"
    for key in "$@"; do
      awk -v lock="$lock" -v key="$key" -v version="$resolved" -v platform="$platform" "$velnor_lock_awk" "$lock" >> "$velnor_dir/lock" || velnor_fail "$lock could not be read"
    done
  done
  lock_digest=unlocked
  if [ -s "$velnor_dir/lock" ]; then
    if command -v sha256sum >/dev/null 2>&1; then
      lock_digest=$(sha256sum < "$velnor_dir/lock") || velnor_fail "$name: lock digest failed"
    else
      lock_digest=$(shasum -a 256 < "$velnor_dir/lock") || velnor_fail "$name: lock digest failed"
    fi
    lock_digest=${lock_digest%% *}
    case "$lock_digest" in
      *[!0-9a-f]*) velnor_fail "$name: lock digest is not hexadecimal" ;;
    esac
    [ "${#lock_digest}" -eq 64 ] || velnor_fail "$name: lock digest is not SHA-256"
  fi
  printf '%s=%s\n' "$name" "$lock_digest" >> "$GITHUB_ENV"
  printf 'velnor: %s=%s\n' "$name" "$lock_digest"
}
velnor_locks='mise.lock .mise.lock'
for velnor_file in mise.toml .mise.toml mise.*.toml .mise.*.toml .config/mise.toml .config/mise.*.toml .config/mise/config.toml .config/mise/config.*.toml .config/mise/conf.d/*.toml mise/config.toml mise/config.*.toml .mise/config.toml .mise/config.*.toml; do
  if [ ! -e "$velnor_file" ] && [ ! -L "$velnor_file" ]; then continue; fi
  velnor_regular "$velnor_file"
  velnor_tools "$velnor_file"
done
if [ -e .tool-versions ] || [ -L .tool-versions ]; then
  velnor_regular .tool-versions
  velnor_tool_versions .tool-versions
fi
if [ -e rust-toolchain.toml ] || [ -L rust-toolchain.toml ]; then
  velnor_regular rust-toolchain.toml
  velnor_toolchain rust-toolchain.toml
fi
if [ -e rust-toolchain ] || [ -L rust-toolchain ]; then
  velnor_regular rust-toolchain
  velnor_toolchain rust-toolchain
fi
velnor_tool VELNOR_TOOL_BUN 'bun' 'core:bun'
velnor_tool VELNOR_TOOL_JACTIONLINT 'jactionlint' 'github:jdx/jactionlint' 'packslip:github.com/jdx/jactionlint'
velnor_tool VELNOR_TOOL_RUST 'rust' 'core:rust'
velnor_tool VELNOR_TOOL_SHELLCHECK 'shellcheck' 'aqua:koalaman/shellcheck'
rm -rf "$velnor_dir"
