# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
# Setup Mise already put the restored `bin/` and `shims/` on PATH ahead of
# the system directories, so every helper comes from root-owned system
# directories only (the step's shell is an absolute path for the same reason).
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
root="$HOME/.local/share/mise"
installs="$root/installs"
archives="$installs/.velnor-verified-archives"
lock="${GITHUB_WORKSPACE:-$PWD}/mise.lock"
# Code Mise could run that no lock pins: rebuilt on demand, never restored.
rm -rf "$root/plugins" "$root/shims"
[ ! -L "$installs" ] || rm -f "$installs"
[ -d "$installs" ] || exit 0
[ ! -L "$archives" ] || rm -f "$archives"
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
origin_fetches=0
origin_bytes=0
origin_bytes_unknown=
reused_archives=0
verified_files=0
verification_failed=0
# `<install dir name> <version> <url> <checksum>` for this platform.
: > "$work/pins"
: > "$work/retained"
catalog_pins() { printf '%s\n' "${VELNOR_TOOL_PINS:-}" | awk -v p="$platform" 'NF == 5 && $3 == p { print $1, $2, $4, "sha256:" $5 }'; }
lock_pins() {
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
  ' "$lock"
}
[ -z "$platform" ] || catalog_pins > "$work/pins"
cp "$work/pins" "$work/catalog-pins"
if [ -n "$platform" ] && [ -f "$lock" ] && [ ! -L "$lock" ]; then lock_pins >> "$work/pins"; fi
discard() { printf 'velnor: discarding restored %s: %s; Prepare pinned tools reinstalls it\n' "$1" "$2"; rm -rf "$3"; }
file_size() { stat -c '%s' "$1" 2>/dev/null || stat -f '%z' "$1"; }
mapping_key() {
  awk '
    { text = text $0 "\n" }
    END {
      ws = "[ \t\r\n]*"
      str = "\"[^\"\\\\[:cntrl:]]*\""
      urls = "\\[" ws "(" str "(" ws "," ws str ")*)?" ws "\\]"
      pattern = "^" ws "\\{" ws "\"blake3\"" ws ":" ws "\"[0-9a-f]+\"" ws "," ws "\"effective_filename\"" ws ":" ws "(" str "|null)" ws "," ws "\"urls\"" ws ":" ws urls ws "\\}" ws "$"
      if (text ~ /[^\t\r\n -~]/ || text !~ pattern) exit 1
      split(text, fields, "\"")
      print fields[4]
    }
  ' "$1"
}
retain_download() {
  local sum=$1 stored=$2 cache cas mapping key file size valid dir
  [ "${VELNOR_TOOL_PRODUCER:-false}" = true ] || return 1
  for cache in "$HOME/.cache/mise" "$HOME/Library/Caches/mise"; do
    cas="$cache/downloads-cas"
    valid=true
    for dir in "$cache" "$cas" "$cas/blobs"; do
      [ -d "$dir" ] && [ ! -L "$dir" ] || valid=false
    done
    [ "$valid" = true ] || continue
    for dir in "$cas/pins" "$cas/pins/sha256"; do
      [ ! -L "$dir" ] && { [ ! -e "$dir" ] || [ -d "$dir" ]; } || return 1
    done
    mapping="$cas/pins/sha256/$sum.json"
    if [ ! -e "$mapping" ] && [ ! -L "$mapping" ]; then
      file=$(bounded_download "$cas" "$sum") || return 1
    else
    [ -f "$mapping" ] && [ ! -L "$mapping" ] || return 1
    size=$(file_size "$mapping") || return 1
    [ "$size" -le 65536 ] || return 1
    key=$(mapping_key "$mapping") || return 1
    case "$key" in ''|*[!0-9a-f]*) return 1 ;; esac
    [ "${#key}" -eq 64 ] || return 1
    [ -d "$cas/blobs/$key" ] && [ ! -L "$cas/blobs/$key" ] || return 1
    file="$cas/blobs/$key/artifact"
    [ -f "$file" ] && [ ! -L "$file" ] || return 1
    size=$(file_size "$file") || return 1
    [ "$size" -le 1610612736 ] || return 1
    [ "$("${h[@]}" "$file" | cut -d" " -f1)" = "$sum" ] || return 1
    fi
    mkdir -p "$archives" || return 1
    cp "$file" "$stored" || return 1
    return 0
  done
  return 1
}
bounded_download() {
  local cas=$1 sum=$2 deadline=$((SECONDS + 10)) count total=0 dir key file size pid result
  if find "$cas/blobs" -mindepth 1 -maxdepth 1 -print | head -n 65 > "$work/candidates"; then :
  else [ "$(wc -l < "$work/candidates")" -eq 65 ] || return 1; fi
  count=$(wc -l < "$work/candidates")
  [ "$count" -le 64 ] || return 1
  : > "$work/artifacts"
  while IFS= read -r dir; do
    key=${dir##*/}
    case "$key" in ''|*[!0-9a-f]*) return 1 ;; esac
    [ "${#key}" -eq 64 ] && [ -d "$dir" ] && [ ! -L "$dir" ] || return 1
    file="$dir/artifact"
    [ -f "$file" ] && [ ! -L "$file" ] || return 1
    size=$(file_size "$file") || return 1
    total=$((total + size))
    [ "$size" -le 1610612736 ] && [ "$total" -le 1610612736 ] || return 1
    printf '%s\n' "$file" >> "$work/artifacts"
  done < "$work/candidates"
  while IFS= read -r file; do
    [ "$SECONDS" -lt "$deadline" ] || return 1
    "${h[@]}" "$file" > "$work/hash" &
    pid=$!
    while kill -0 "$pid" 2>/dev/null; do
      if [ "$SECONDS" -ge "$deadline" ]; then
        echo "download CAS hash deadline exceeded" >&2
        kill -KILL "$pid" 2>/dev/null || :
        if wait "$pid"; then :; fi
        return 1
      fi
      sleep 0.1
    done
    wait "$pid" || return 1
    result=$(cut -d" " -f1 "$work/hash")
    if [ "$result" = "$sum" ]; then printf '%s\n' "$file"; return 0; fi
  done < "$work/artifacts"
  return 1
}
tree_modes() {
  local file mode
  while IFS= read -r -d '' file; do
    mode=$(stat -c '%a' "$file" 2>/dev/null || stat -f '%Lp' "$file") || return 1
    printf '%s %q\n' "$mode" "$file"
  done < <(find . -type f ! -path './.mise-*' -print0)
}
verify_install() {
  local dir=$1 tool=$2 version=$3 url sum line archive x link target stored size n
  case "$tool$version" in *[!A-Za-z0-9._+-]*) echo "unsafe name"; return 1 ;; esac
  line=$(awk -v t="$tool" -v v="$version" '$1 == t && $2 == v { print $3, $4; exit }' "$work/pins")
  [ -n "$line" ] || { echo "no mise.lock or catalog pin for ${platform:-this runner}"; return 2; }
  url=${line%% *}; sum=${line#* }
  case "$sum" in sha256:*) sum=${sum#sha256:} ;; *) echo "mise.lock checksum is not SHA-256"; return 1 ;; esac
  case "$url" in https://*) ;; *) echo "mise.lock URL is not HTTPS"; return 1 ;; esac
  case "$sum" in '' | *[!0-9a-f]*) echo "mise.lock checksum is not SHA-256"; return 1 ;; esac
  [ "${#sum}" -eq 64 ] || { echo "mise.lock checksum is not SHA-256"; return 1; }
  stored="$archives/$sum"
  archive="$work/archive"
  rm -rf "$archive" "$work/x"; mkdir -p "$work/x"
  if [ ! -f "$stored" ] && [ "${VELNOR_TOOL_PRODUCER:-false}" = true ]; then
    retain_download "$sum" "$stored" || { echo "verified Mise download is unavailable or invalid"; return 1; }
  fi
  if [ -f "$stored" ] && [ ! -L "$stored" ] && [ "$("${h[@]}" "$stored" | cut -d" " -f1)" = "$sum" ]; then
    printf 'velnor: reusing verified %s@%s archive; no origin download\n' "$tool" "$version"
    archive="$stored"
    reused_archives=$((reused_archives + 1))
  else
    rm -f "$stored"
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail --silent --show-error --location --retry 3 --output "$archive" "$url" \
      || { echo "download failed"; return 1; }
    [ "$("${h[@]}" "$archive" | cut -d" " -f1)" = "$sum" ] || { echo "archive does not match the mise.lock checksum"; return 1; }
    origin_fetches=$((origin_fetches + 1))
    size=$(wc -c < "$archive" | tr -d ' ')
    case "$size" in '' | *[!0-9]*) origin_bytes_unknown=1 ;; *) origin_bytes=$((origin_bytes + size)) ;; esac
    mkdir -p "$archives"
    cp "$archive" "$stored"
  fi
  case "$url" in
    *.tar.gz|*.tgz) tar -xzf "$archive" -C "$work/x" ;;
    *.tar.xz|*.txz) tar -xJf "$archive" -C "$work/x" ;;
    *.tar.bz2|*.tbz2) tar -xjf "$archive" -C "$work/x" ;;
    *.tar) tar -xf "$archive" -C "$work/x" ;;
    *.zip) unzip -q "$archive" -d "$work/x" ;;
    *.gz) gunzip -c "$archive" > "$work/x/file" ;;
    *) cp "$archive" "$work/x/file" ;;
  esac || { echo "archive does not extract"; return 1; }
  if [ "$tool" = jactionlint ]; then rm -rf "$dir/.mise-bins"; fi
  (cd "$work/x" && find . -type f -exec "${h[@]}" {} +) | cut -d' ' -f1 | LC_ALL=C sort -u > "$work/archive.sums"
  if [ -n "$(find "$dir" ! -type f ! -type d ! -type l -print -quit)" ]; then echo "special file"; return 1; fi
  while IFS= read -r -d '' link; do
    target=$(readlink "$link") || { echo "unreadable symlink"; return 1; }
    case "$target" in
      ..|../*|*/../*|*/..) echo "symlink ${link#"$dir"/} leaves the install"; return 1 ;;
      "$dir"/*) ;;
      /*) echo "symlink ${link#"$dir"/} leaves the install"; return 1 ;;
    esac
  done < <(find "$dir" -type l -print0)
  # Mise's own bookkeeping under `.mise-*` (the packslip statement, shell
  # completions, `.mise-bins` links checked above) is data that CI never runs;
  # it may hold no executable file.
  if [ -n "$(cd "$dir" && find . -path './.mise-*' -type f -perm -u+x -print -quit)" ]; then
    echo "executable file in Mise bookkeeping"; return 1
  fi
  (cd "$dir" && find . -type f ! -path './.mise-*' -exec "${h[@]}" {} +) \
    | cut -d' ' -f1 | LC_ALL=C sort -u > "$work/install.sums"
  x=$(LC_ALL=C comm -23 "$work/install.sums" "$work/archive.sums" | head -n 1)
  [ -z "$x" ] || { echo "a file differs from the pinned archive"; return 1; }
  case "$tool" in
    shellcheck|gh)
      (cd "$work/x" && find . -type f -exec "${h[@]}" {} +) | LC_ALL=C sort > "$work/archive.paths"
      (cd "$dir" && find . -type f ! -path './.mise-*' -exec "${h[@]}" {} +) | LC_ALL=C sort > "$work/install.paths"
      cmp -s "$work/archive.paths" "$work/install.paths" || { echo "installed paths differ from the pinned archive"; return 1; }
      (cd "$work/x" && tree_modes) | LC_ALL=C sort > "$work/archive.exec"
      (cd "$dir" && tree_modes) | LC_ALL=C sort > "$work/install.exec"
      cmp -s "$work/archive.exec" "$work/install.exec" || { echo "installed executable modes differ from the pinned archive"; return 1; }
      ;;
  esac
  n=$(wc -l < "$work/install.sums" | tr -d ' ')
  case "$n" in '' | *[!0-9]*) ;; *) verified_files=$((verified_files + n)) ;; esac
  verified_archive=$sum
}
# Every entry, dot-named ones included (a glob skips those), is verified or
# discarded: tool directories, version directories and aliases alike.
entries() { find "$1" -mindepth 1 -maxdepth 1 -print0; }
while IFS= read -r -d '' tool_dir; do
  [ -d "$tool_dir" ] && [ ! -L "$tool_dir" ] || { rm -rf "$tool_dir"; continue; }
  tool=${tool_dir##*/}
  case "$tool" in rust|core-rust|.velnor-verified-archives) continue ;; esac
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
    if verify_install "$dir" "$tool" "$version" >"$work/verify.out" 2>&1; then
      cat "$work/verify.out"
      printf '%s\n' "$verified_archive" >> "$work/retained"
      verified="$verified$version "
      printf 'velnor: restored %s@%s matches its mise.lock archive\n' "$tool" "$version"
    else
      status=$?
      reason=$(cat "$work/verify.out")
      if ! awk -v t="$tool" -v v="$version" '$1 == t && $2 == v { found = 1 } END { exit !found }' "$work/catalog-pins"; then
        status=2
      fi
      discard "$tool@$version" "${reason:-verification failed}" "$dir"
      [ "$status" -eq 2 ] || verification_failed=1
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
if [ "${VELNOR_TOOL_PRODUCER:-false}" = true ] && [ -d "$archives" ]; then
  while IFS= read -r -d '' archive; do
    sum=${archive##*/}
    if ! awk -v sum="$sum" '$0 == sum { found = 1 } END { exit !found }' "$work/retained"; then rm -rf "$archive"; fi
  done < <(entries "$archives")
fi
if [ -n "${GITHUB_OUTPUT-}" ]; then
  [ -n "$origin_bytes_unknown" ] || printf 'origin-bytes=%s\n' "$origin_bytes" >> "$GITHUB_OUTPUT"
  printf 'origin-fetches=%s\nreused-archives=%s\nverified-files=%s\n' "$origin_fetches" "$reused_archives" "$verified_files" >> "$GITHUB_OUTPUT"
fi
if [ "${VELNOR_TOOL_PRODUCER:-false}" = true ] && [ "$verification_failed" -ne 0 ]; then exit 1; fi
if [ -n "${GITHUB_STEP_SUMMARY-}" ]; then
  {
    printf 'velnor tools verify: origin-fetches=%s ' "$origin_fetches"
    [ -n "$origin_bytes_unknown" ] || printf 'origin-bytes=%s ' "$origin_bytes"
    printf 'reused-archives=%s verified-files=%s\n' "$reused_archives" "$verified_files"
  } >> "$GITHUB_STEP_SUMMARY"
fi
