# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
# Re-verify every Mise install the tools cache restored, before anything runs
# one. A cached install is kept only when every file in it is byte-identical
# to a file of the release archive `mise.lock` (or, for a tool without a lock
# entry, the Velnor catalog) pins for this platform (the
# archive is downloaded and its SHA-256 checked first) and every symlink in
# it stays inside it. Anything else (no lock entry, another checksum
# algorithm, a changed or planted file, an escaping symlink) is discarded, so
# `Prepare pinned tools` reinstalls it from the lock. Rust is checked by its
# own step (`Verify restored Rust toolchain`).
#
# Setup Mise already put the restored `bin/` and `shims/` on PATH ahead of
# the system directories, so every helper comes from root-owned system
# directories only (the step's shell is an absolute path for the same reason).
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
root="$HOME/.local/share/mise"
installs="$root/installs"
lock="${GITHUB_WORKSPACE:-$PWD}/mise.lock"
# Code Mise could run that no lock pins: rebuilt on demand, never restored.
rm -rf "$root/plugins" "$root/shims"
[ ! -L "$installs" ] || rm -f "$installs"
[ -d "$installs" ] || exit 0
case "${RUNNER_OS:-}-${RUNNER_ARCH:-}" in
  Linux-X64) platform=linux-x64 ;;
  Linux-ARM64) platform=linux-arm64 ;;
  macOS-ARM64) platform=macos-arm64 ;;
  macOS-X64) platform=macos-x64 ;;
  *) platform= ;;
esac
if command -v sha256sum >/dev/null 2>&1; then h=(sha256sum); else h=(shasum -a 256); fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# `<install dir name> <version> <url> <checksum>` for this platform.
: > "$work/pins"
if [ -n "$platform" ] && [ -f "$lock" ] && [ ! -L "$lock" ]; then
  awk -v want="platforms.$platform" '
    function unq(s) { gsub(/^"|"$/, "", s); return s }
    function flush() { if (section && url != "" && sum != "") print dir, version, url, sum; section = 0; url = ""; sum = "" }
    /^\[\[tools\./ { flush(); name = $0; sub(/^\[\[tools\./, "", name); sub(/\]\]$/, "", name); name = unq(name); dir = name; gsub(/[:\/]/, "-", dir); version = ""; next }
    /^\[tools\./ { flush(); s = $0; n = index(s, "\"platforms."); if (n > 0) { p = substr(s, n + 1); sub(/"\]$/, "", p); section = (p == want) } ; next }
    /^\[/ { flush(); next }
    $1 == "version" && !section { version = unq($3) }
    section && $1 == "url" { url = unq($3) }
    section && $1 == "checksum" { sum = unq($3) }
    END { flush() }
  ' "$lock" > "$work/pins"
fi
# Velnor catalog pins (`<dir> <version> <platform> <url> <sha256>`) for tools
# Velnor installs itself, used when `mise.lock` has no entry.
if [ -n "$platform" ]; then
  printf '%s\n' "${VELNOR_TOOL_PINS:-}" | awk -v p="$platform" 'NF == 5 && $3 == p { print $1, $2, $4, "sha256:" $5 }' >> "$work/pins"
fi
discard() { printf 'velnor: discarding restored %s: %s; Prepare pinned tools reinstalls it\n' "$1" "$2"; rm -rf "$3"; }
verify_install() {
  local dir=$1 tool=$2 version=$3 url sum line archive x link target
  line=$(awk -v t="$tool" -v v="$version" '$1 == t && $2 == v { print $3, $4; exit }' "$work/pins")
  [ -n "$line" ] || { echo "no mise.lock or catalog pin for ${platform:-this runner}"; return 1; }
  url=${line%% *}; sum=${line#* }
  case "$sum" in sha256:*) sum=${sum#sha256:} ;; *) echo "mise.lock checksum is not SHA-256"; return 1 ;; esac
  case "$url" in https://*) ;; *) echo "mise.lock URL is not HTTPS"; return 1 ;; esac
  archive="$work/archive"
  rm -rf "$archive" "$work/x"; mkdir -p "$work/x"
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error --location --retry 3 --output "$archive" "$url" \
    || { echo "download failed"; return 1; }
  [ "$("${h[@]}" "$archive" | cut -d" " -f1)" = "$sum" ] || { echo "archive does not match the mise.lock checksum"; return 1; }
  case "$url" in
    *.tar.gz|*.tgz) tar -xzf "$archive" -C "$work/x" ;;
    *.tar.xz|*.txz) tar -xJf "$archive" -C "$work/x" ;;
    *.tar.bz2|*.tbz2) tar -xjf "$archive" -C "$work/x" ;;
    *.tar) tar -xf "$archive" -C "$work/x" ;;
    *.zip) unzip -q "$archive" -d "$work/x" ;;
    *.gz) gunzip -c "$archive" > "$work/x/file" ;;
    *) cp "$archive" "$work/x/file" ;;
  esac || { echo "archive does not extract"; return 1; }
  (cd "$work/x" && find . -type f -exec "${h[@]}" {} +) | cut -d' ' -f1 | LC_ALL=C sort -u > "$work/archive.sums"
  if [ -n "$(find "$dir" ! -type f ! -type d ! -type l -print -quit)" ]; then echo "special file"; return 1; fi
  while IFS= read -r link; do
    target=$(readlink "$link")
    case "$target" in
      "$dir"/*) ;;
      /*|..|../*|*/../*|*/..) echo "symlink ${link#"$dir"/} leaves the install"; return 1 ;;
    esac
  done < <(find "$dir" -type l)
  # Mise's own bookkeeping under `.mise-*` (the packslip statement, shell
  # completions, `.mise-bins` links checked above) is data that CI never runs;
  # it may hold no executable file.
  if [ -n "$(find "$dir" -path "$dir/.mise-*" -type f -perm -u+x -print -quit)" ]; then
    echo "executable file in Mise bookkeeping"; return 1
  fi
  (cd "$dir" && find . -type f ! -path './.mise-*' -exec "${h[@]}" {} +) \
    | cut -d' ' -f1 | LC_ALL=C sort -u > "$work/install.sums"
  x=$(LC_ALL=C comm -23 "$work/install.sums" "$work/archive.sums" | head -n 1)
  [ -z "$x" ] || { echo "a file differs from the pinned archive"; return 1; }
}
# Every entry, dot-named ones included (a glob skips those), is verified or
# discarded: tool directories, version directories and aliases alike.
entries() { find "$1" -mindepth 1 -maxdepth 1 -print0; }
while IFS= read -r -d '' tool_dir; do
  [ -d "$tool_dir" ] && [ ! -L "$tool_dir" ] || { rm -rf "$tool_dir"; continue; }
  tool=${tool_dir##*/}
  case "$tool" in rust|core-rust) continue ;; esac
  # Real version directories first, so an alias is judged by a verified one.
  verified=" "
  while IFS= read -r -d '' dir; do
    [ ! -L "$dir" ] || continue
    version=${dir##*/}
    if [ ! -d "$dir" ]; then
      # Mise's per-tool backend record is data; any other file is dropped.
      [ "$version" = .mise.backend.toml ] && [ -f "$dir" ] && [ ! -x "$dir" ] || rm -rf "$dir"
      continue
    fi
    if reason=$(verify_install "$dir" "$tool" "$version"); then
      verified="$verified$version "
      printf 'velnor: restored %s@%s matches its mise.lock archive\n' "$tool" "$version"
    else
      discard "$tool@$version" "${reason:-verification failed}" "$dir"
    fi
  done < <(entries "$tool_dir")
  # A version alias (`1.26 -> ./1.26.0`) is kept only when it names a sibling
  # version directory verified above; a chain, a path or a dangling link is not.
  while IFS= read -r -d '' dir; do
    [ -L "$dir" ] || continue
    target=$(readlink "$dir")
    target=${target#./}
    case "$target" in ''|.|..|*/*) rm -f "$dir"; continue ;; esac
    case "$verified" in *" $target "*) ;; *) rm -f "$dir"; continue ;; esac
    [ -d "$tool_dir/$target" ] && [ ! -L "$tool_dir/$target" ] || rm -f "$dir"
  done < <(entries "$tool_dir")
done < <(entries "$installs")
