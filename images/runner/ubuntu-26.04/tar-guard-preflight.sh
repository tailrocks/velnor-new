#!/usr/bin/env bash
# Validate selected archive members before the pinned GNU tar backend extracts.
set -euo pipefail

guard_dir=/usr/local/libexec/velnor
archive=""
destination=""
selectors_file=""
selected=0
absolute=0
compressor=""
program=""
i=0
while [ "$i" -lt "$#" ]; do
  case "$1" in
    --archive) archive="$2"; shift 2 ;;
    --destination) destination="$2"; shift 2 ;;
    --selectors) selectors_file="$2"; shift 2 ;;
    --selected) selected=1; shift ;;
    --absolute) absolute=1; shift ;;
    --compressor) compressor="$2"; shift 2 ;;
    --program) program="$2"; shift 2 ;;
    *) printf 'velnor-tar: invalid preflight argument\n' >&2; exit 2 ;;
  esac
done
[ -n "$archive" ] && [ -n "$destination" ] && [ -n "$selectors_file" ] || {
  printf 'velnor-tar: incomplete preflight request\n' >&2
  exit 2
}

work="$(mktemp -d "${TMPDIR:-/tmp}/velnor-tar-preflight.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
records="$work/members"
if [ -z "$compressor" ] && [ -z "$program" ]; then
  magic="$(od -An -N9 -tx1 -- "$archive" | tr -d ' \n')"
  case "$magic" in
    1f8b*) compressor=gzip ;;
    425a68*) compressor=bzip2 ;;
    fd377a585a00*) compressor=xz ;;
    28b52ffd* | 5[0-9a-f]2a4d18*) compressor=zstd ;;
    5d*) compressor=lzma ;;
    4c5a4950*) compressor=lzip ;;
    894c5a4f000d0a1a0a*) compressor=lzop ;;
    1f9d* | 1fa0*) compressor=compress ;;
  esac
  if [ -z "$compressor" ]; then
    case "$archive" in
      *.gz | *.tgz | *.taz) compressor=gzip ;;
      *.Z | *.taZ) compressor=compress ;;
      *.bz2 | *.tz2 | *.tbz2 | *.tbz) compressor=bzip2 ;;
      *.lz) compressor=lzip ;;
      *.lzma | *.tlz) compressor=lzma ;;
      *.lzo) compressor=lzop ;;
      *.xz) compressor=xz ;;
      *.zst | *.tzst) compressor=zstd ;;
    esac
  fi
fi
if [ -n "$program" ]; then
  set -o pipefail
  /bin/sh -c "$program -d" <"$archive" | perl "$guard_dir/tar-member.pl" --list >"$records"
else
  set -o pipefail
  case "$compressor" in
    "") cat -- "$archive" ;;
    gzip) gzip -dc <"$archive" ;;
    compress)
      if command -v compress >/dev/null 2>&1; then
        compress -dc <"$archive"
      else
        gzip -dc <"$archive"
      fi
      ;;
    bzip2)
      if command -v bzip2 >/dev/null 2>&1; then
        bzip2 -dc <"$archive"
      else
        lbzip2 -dc <"$archive"
      fi
      ;;
    xz) xz -dc <"$archive" ;;
    lzma)
      if command -v lzma >/dev/null 2>&1; then
        lzma -dc <"$archive"
      else
        xz -dc <"$archive"
      fi
      ;;
    lzip) lzip -dc <"$archive" ;;
    lzop) lzop -dc <"$archive" ;;
    zstd) zstd -dc <"$archive" ;;
    *) printf 'velnor-tar: unsupported archive compression\n' >&2; exit 2 ;;
  esac | perl "$guard_dir/tar-member.pl" --list >"$records"
fi

declare -a fields=() member_name=() member_type=() member_link=() member_selected=()
mapfile -t fields <"$records"
[ "$(( ${#fields[@]} % 5 ))" -eq 0 ] || {
  printf 'velnor-tar: malformed member index\n' >&2
  exit 2
}
for ((i = 0; i < ${#fields[@]}; i += 5)); do
  member_type+=("${fields[$((i + 1))]}")
  member_link+=("${fields[$((i + 2))]}")
  member_name+=("${fields[$((i + 3))]}")
  member_selected+=(0)
done

# GNU tar treats leading ./, duplicate separators, and trailing directory
# slashes as the same selection name. Preflight every such archive alias.
member_key() {
  local input="$1" part result="" absolute=0
  local -a parts=() stack=()
  [[ "$input" == /* ]] && absolute=1
  IFS=/ read -r -a parts <<<"$input"
  for part in "${parts[@]}"; do
    case "$part" in
      "" | .) ;;
      *) stack+=("$part") ;;
    esac
  done
  for part in "${stack[@]}"; do
    [ -n "$result" ] && result+="/"
    result+="$part"
  done
  if [ "$absolute" -eq 1 ]; then
    printf '/%s' "$result"
  else
    printf '%s' "${result:-.}"
  fi
}

declare -a selectors=()
while IFS= read -r -d '' selector; do
  selectors+=("$selector")
done <"$selectors_file"

if [ "$selected" -eq 0 ]; then
  for ((i = 0; i < ${#member_name[@]}; i++)); do
    member_selected[$i]=1
  done
else
  declare -A selector_keys=() selector_found=() selected_prefixes=()
  for selector in "${selectors[@]}"; do
    selector_key="$(member_key "$selector")"
    selector_keys["$selector_key"]=1
    selector_found["$selector_key"]=0
  done
  for ((i = 0; i < ${#member_name[@]}; i++)); do
    name_key="$(member_key "${member_name[$i]}")"
    if [ -n "${selector_keys[$name_key]:-}" ]; then
      member_selected[$i]=1
      selector_found["$name_key"]=1
      selected_prefixes["$name_key"]=1
    fi
  done
  for selector_key in "${!selector_keys[@]}"; do
    [ "${selector_found[$selector_key]}" -eq 1 ] || {
      printf 'velnor-tar: selected member is absent from archive\n' >&2
      exit 2
    }
  done
  # GNU tar recursively selects descendants of a matched pathname even when
  # the matched header is a symlink, so preflight must inspect those children.
  for ((i = 0; i < ${#member_name[@]}; i++)); do
    name="$(member_key "${member_name[$i]}")"
    if [ -n "${selected_prefixes[/]:-}" ] && [[ "$name" == /* ]]; then
      member_selected[$i]=1
    elif [ -n "${selected_prefixes[.]:-}" ] && [[ "$name" != /* ]]; then
      member_selected[$i]=1
    else
      parent="$name"
      while [[ "$parent" == */* ]]; do
        parent="${parent%/*}"
        [ -n "$parent" ] || parent=/
        if [ -n "${selected_prefixes[$parent]:-}" ]; then
          member_selected[$i]=1
          break
        fi
        [ "$parent" != / ] || break
      done
    fi
  done
fi

norm_path() {
  local input="$1" part result=""
  local -a parts=()
  if [[ "$input" != /* ]]; then input="$PWD/$input"; fi
  IFS=/ read -r -a parts <<<"$input"
  local -a stack=()
  for part in "${parts[@]}"; do
    case "$part" in
      "" | .) ;;
      ..)
        if [ "${#stack[@]}" -gt 0 ]; then
          unset "stack[$((${#stack[@]} - 1))]"
        fi
        ;;
      *) stack+=("$part") ;;
    esac
  done
  for part in "${stack[@]}"; do result+="/$part"; done
  printf '%s' "${result:-/}"
}

path_under() {
  local root="$1" path="$2"
  [ "$root" = / ] && return 0
  [ "$path" = "$root" ] || [[ "$path" == "$root"/* ]]
}

member_intended() {
  local name="$1"
  if [[ "$name" == /* ]]; then norm_path "$name"; else norm_path "$root/$name"; fi
}

job_path() {
  local path="$1"
  path_under "$root" "$path" || path_under /home/runner/work "$path" || \
    path_under /home/runner/_work "$path"
}

root="$(realpath -e -- "$destination")" || {
  printf 'velnor-tar: extraction directory cannot be resolved\n' >&2
  exit 2
}
if [ "$absolute" -eq 0 ]; then
  for name in "${member_name[@]}"; do
    case "$name" in
      /* | .. | ../* | */.. | */../*)
        printf 'velnor-tar: member path requires explicit -P\n' >&2
        exit 2
        ;;
    esac
  done
fi

declare -A archive_links=()
for ((i = 0; i < ${#member_name[@]}; i++)); do
  [ "${member_selected[$i]}" -eq 1 ] || continue
  [ "${member_type[$i]}" = 2 ] || continue
  name="${member_name[$i]}"
  key="$(member_intended "$name")"
  if ! job_path "$key"; then
    printf 'velnor-tar: selected symlink path is outside allowed roots\n' >&2
    exit 2
  fi
  archive_links["$key"]=1
  target="${member_link[$i]}"
  [ -n "$target" ] || {
    printf 'velnor-tar: selected symlink has an empty target\n' >&2
    exit 2
  }
  if [[ "$target" == /* ]]; then
    intended="$(norm_path "$target")"
  else
    dir="${key%/*}"
    [ -n "$dir" ] || dir=/
    intended="$(norm_path "$dir/$target")"
  fi
  if ! job_path "$intended" && ! { [ "$absolute" -eq 1 ] && [[ "$target" != /* ]]; }; then
    printf 'velnor-tar: selected symlink target is outside allowed roots\n' >&2
    exit 2
  fi
done

has_symlink_ancestor() {
  local name="$1" current rest part last index
  local -a parts=()
  if [[ "$name" == /* ]]; then
    current=/
    rest="${name#/}"
  else
    current="$root"
    rest="$name"
  fi
  IFS=/ read -r -a parts <<<"$rest"
  last=$(( ${#parts[@]} - 1 ))
  for ((index = 0; index < ${#parts[@]}; index++)); do
    part="${parts[$index]}"
    case "$part" in
      "" | .) continue ;;
      ..)
        [ "$current" = / ] || current="${current%/*}"
        [ -n "$current" ] || current=/
        ;;
      *)
        current="${current%/}/$part"
        if [ "$index" -lt "$last" ] && \
          { [ -n "${archive_links[$current]:-}" ] || [ -L "$current" ]; }; then
          return 0
        fi
        ;;
    esac
  done
  return 1
}

for ((i = 0; i < ${#member_name[@]}; i++)); do
  [ "${member_selected[$i]}" -eq 1 ] || continue
  if has_symlink_ancestor "${member_name[$i]}"; then
    printf 'velnor-tar: selected member traverses a symlink\n' >&2
    exit 2
  fi
done
