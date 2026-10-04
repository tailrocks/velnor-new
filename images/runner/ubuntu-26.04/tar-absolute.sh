# Sourced by tar-shim.sh. BusyBox strips leading "/" and ".." and rejects -P.
# GNU tar cannot stat or open those paths under qemu-user (openat2 ENOSYS).
# -P extract: stage each stripped member by archive index, then move that
# inode to the path GNU -P would have opened. -P create uses velnor-tar-pax.

strip_unsafe() {
  local s="$1"
  while true; do
    if [[ "$s" == /* ]]; then
      s="${s#/}"
      continue
    fi
    if [[ "$s" == ../* ]]; then
      s="${s#../}"
      continue
    fi
    case "$s" in
      */../*)
        s="${s#*/../}"
        continue
        ;;
    esac
    break
  done
  printf '%s' "$s"
}

norm_path() {
  local input="$1"
  local abs=0
  local -a stack=()
  local rest="$input"
  local part
  [[ "$input" == /* ]] && abs=1
  while [ -n "$rest" ]; do
    part="${rest%%/*}"
    if [ "$rest" = "$part" ]; then
      rest=""
    else
      rest="${rest#*/}"
    fi
    case "$part" in
      "" | ".") ;;
      "..")
        if [ "${#stack[@]}" -gt 0 ] && [ "${stack[$((${#stack[@]} - 1))]}" != ".." ]; then
          stack=("${stack[@]:0:$((${#stack[@]} - 1))}")
        elif [ "$abs" -eq 0 ]; then
          stack+=("..")
        fi
        ;;
      *) stack+=("$part") ;;
    esac
  done
  if [ "$abs" -eq 1 ]; then
    if [ "${#stack[@]}" -eq 0 ]; then
      printf '/'
      return
    fi
    local out="" p
    for p in "${stack[@]}"; do
      out="$out/$p"
    done
    printf '%s' "$out"
    return
  fi
  if [ "${#stack[@]}" -eq 0 ]; then
    printf '.'
    return
  fi
  local IFS=/
  printf '%s' "${stack[*]}"
}

member_intended() {
  local base
  if [[ "$1" == /* ]]; then
    norm_path "$1"
    return
  fi
  base="${chdir:-$PWD}"
  norm_path "$base/$1"
}

run_busybox() {
  run_members
}

# One BusyBox invocation. Extra arguments are member names for this batch only.
run_members() {
  if [ -n "$program" ]; then
    if [ "$mode" = c ]; then
      "${bb[@]}" "$@" | bash -c "$program" >"$archive"
    else
      bash -c "$program" <"$archive" | "${bb[@]}" "$@"
    fi
  else
    "${bb[@]}" "$@"
  fi
}

# Extract or list the file list in batches so one exec stays under ARG_MAX.
run_listed() {
  local line list batch=()
  list="$(mktemp)"
  if [ "${#filtered[@]}" -gt 0 ]; then
    printf '%s\n' "${filtered[@]}" >"$list"
  fi
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || continue
    batch+=("$line")
    if [ "${#batch[@]}" -ge 32 ]; then
      run_members "${batch[@]}"
      batch=()
    fi
  done <"$list"
  rm -f "$list"
  if [ "${#batch[@]}" -gt 0 ]; then
    run_members "${batch[@]}"
  fi
}

write_pax_archive() {
  local pax list
  pax="${_velnor_tar_here}/velnor-tar-pax"
  [ -x "$pax" ] || die "absolute names need $pax"
  [ -n "$archive" ] || die "missing archive"
  list="$(mktemp)"
  printf '%s\n' "${filtered[@]}" >"$list"
  if [ -n "$program" ]; then
    "$pax" --chdir "$chdir" --files-from "$list" | bash -c "$program" >"$archive"
  elif [ "$gzip" -eq 1 ]; then
    "$pax" --chdir "$chdir" --files-from "$list" | gzip -c >"$archive"
  else
    "$pax" --chdir "$chdir" --files-from "$list" >"$archive"
  fi
  rm -f "$list"
}

# Decompressed bytes on stdout. Never a second archive file under /tmp.
stream_archive() {
  [ -n "$archive" ] || die "missing archive"
  if [ -n "$program" ]; then
    bash -c "$program" <"$archive"
  elif [ "$gzip" -eq 1 ]; then
    gzip -dc -- "$archive"
  else
    cat -- "$archive"
  fi
}

list_members() {
  local out="$1"
  # BusyBox tar -t strips "../" before it prints the name. GNU tar lists the
  # stored name and does not open the member, so openat2 is not involved.
  # Same stream as extract: the decompressor is not written to a file.
  # A POSIX locale escapes non-ASCII bytes. Literal quoting keeps the stored name.
  stream_archive | tar.gnu -t --quoting-style=literal -f - >"$out"
}

move_member() {
  local actual="$1"
  local intended="$2"
  if [ -d "$intended" ] && [ ! -L "$intended" ] && [ ! -d "$actual" ]; then
    die "destination is a directory: $intended"
  fi
  mkdir -p -- "$(dirname -- "$intended")"
  if [ -d "$actual" ] && [ ! -L "$actual" ] && [ -d "$intended" ] && [ ! -L "$intended" ]; then
    local child
    for child in "$actual"/* "$actual"/.[!.]* "$actual"/..?*; do
      [ -e "$child" ] || [ -L "$child" ] || continue
      mv -f -- "$child" "$intended"/
    done
    rmdir -- "$actual"
    return
  fi
  mv -f -- "$actual" "$intended"
}

# shellcheck disable=SC1091
. "$_velnor_tar_here/tar-extract.sh"

finish_tar() {
  local needs_pax=0 path stripped
  if [ "$absolute" -eq 1 ] && [ -n "$strip" ]; then
    die "unsupported -P with --strip-components"
  fi
  # actions/cache create passes --posix. That format is the pax writer.
  # Extract and list do not implement it, so they still fail closed.
  if [ "${posix:-0}" -eq 1 ]; then
    if [ "$mode" != c ]; then
      die "unsupported option --posix"
    fi
    write_pax_archive
    return
  fi
  # Every --files-from create is written from that file. BusyBox has no -T,
  # and copying the list onto argv fails once the names exceed ARG_MAX.
  if [ "$mode" = c ] && [ -n "$files_from" ]; then
    write_pax_archive
    return
  fi
  # BusyBox parses a positional member that starts with "-" as an option
  # (`invalid option -- 'd'`). The pax list keeps that name off argv.
  if [ "$mode" = c ] && [ "${#filtered[@]}" -gt 0 ]; then
    for path in "${filtered[@]}"; do
      case "$path" in
        -*)
          write_pax_archive
          return
          ;;
      esac
    done
  fi
  if [ "$absolute" -eq 1 ] && [ "$mode" = c ] && [ "${#filtered[@]}" -gt 0 ]; then
    for path in "${filtered[@]}"; do
      stripped="$(strip_unsafe "$path")"
      if [ "$stripped" != "$path" ]; then
        needs_pax=1
        break
      fi
    done
  fi
  if [ "$mode" = x ]; then
    extract_archive
    return
  fi
  if [ "$needs_pax" -eq 1 ]; then
    write_pax_archive
  elif [ -n "$files_from" ]; then
    run_listed
  else
    run_busybox
  fi
}
