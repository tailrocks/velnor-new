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
root="$HOME/.local/share/mise"
installs="$root/installs"
lock="${GITHUB_WORKSPACE:-$PWD}/mise.lock"
# Code Mise could run that no lock pins: rebuilt on demand, never restored.
rm -rf "$root/plugins" "$root/shims"
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
for tool_dir in "$installs"/*; do
  [ -d "$tool_dir" ] && [ ! -L "$tool_dir" ] || { rm -rf "$tool_dir"; continue; }
  tool=${tool_dir##*/}
  case "$tool" in rust|core-rust) continue ;; esac
  for dir in "$tool_dir"/*; do
    [ -e "$dir" ] || [ -L "$dir" ] || continue
    version=${dir##*/}
    if [ -L "$dir" ]; then
      # Version aliases (`1.26 -> ./1.26.0`) must name a sibling.
      case "$(readlink "$dir")" in ./*/*|/*|*..*) rm -f "$dir" ;; ./*) ;; *) rm -f "$dir" ;; esac
      continue
    fi
    [ -d "$dir" ] || { rm -f "$dir"; continue; }
    if reason=$(verify_install "$dir" "$tool" "$version"); then
      printf 'velnor: restored %s@%s matches its mise.lock archive\n' "$tool" "$version"
    else
      discard "$tool@$version" "${reason:-verification failed}" "$dir"
    fi
  done
done
