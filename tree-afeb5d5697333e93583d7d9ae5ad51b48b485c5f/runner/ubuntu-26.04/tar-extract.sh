# Sourced by tar-absolute.sh. -P members that BusyBox would strip share one
# path, so each of those members is extracted into an index-keyed directory
# and moved alone. A hardlink needs its target in the same BusyBox extract;
# per-member staging cannot keep that inode.

stage_dir=""
raw_temp=""
pack_temp=""
raw_file=""
dest_root=""
isolated_any=0
extract_all=0
member_pl=""

cleanup_extract() {
  if [ -n "${stage_dir}" ]; then
    rm -rf -- "$stage_dir"
    stage_dir=""
  fi
  if [ -n "${raw_temp}" ]; then
    rm -f -- "$raw_temp"
    raw_temp=""
  fi
  if [ -n "${pack_temp}" ]; then
    rm -f -- "$pack_temp"
    pack_temp=""
  fi
}

prepare_raw() {
  [ -n "$archive" ] || die "missing archive"
  raw_file="$archive"
  raw_temp=""
  if [ -z "$program" ] && [ "$gzip" -eq 0 ]; then
    return 0
  fi
  raw_temp="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-raw.XXXXXX")"
  if [ -n "$program" ]; then
    bash -c "$program" <"$archive" >"$raw_temp"
  else
    gzip -dc -- "$archive" >"$raw_temp"
  fi
  raw_file="$raw_temp"
}

path_under() {
  local root="$1"
  local path="$2"
  if [ "$root" = "/" ]; then
    return 0
  fi
  if [ "$path" = "$root" ] || [[ "$path" == "$root"/* ]]; then
    return 0
  fi
  return 1
}

# Without -P, absolute names and ".." segments are untrusted input.
member_rejected() {
  local name="$1"
  local base root intended
  case "$name" in
    /* | .. | ../* | */.. | */../*) return 0 ;;
  esac
  base="${chdir:-$PWD}"
  root="$(norm_path "$base")"
  intended="$(norm_path "$base/$name")"
  if path_under "$root" "$intended"; then
    return 1
  fi
  return 0
}

reject_untrusted() {
  local list member
  list="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-names.XXXXXX")"
  list_members "$list"
  while IFS= read -r member || [ -n "$member" ]; do
    [ -n "$member" ] || continue
    if member_rejected "$member"; then
      rm -f -- "$list"
      die "member escapes destination: $member"
    fi
  done <"$list"
  rm -f -- "$list"
}

member_tool() {
  member_pl="${_velnor_tar_here}/tar-member.pl"
  [ -f "$member_pl" ] || die "absolute names need $member_pl"
}

read_perl_members() {
  local list index type link name
  list="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-list.XXXXXX")"
  perl "$member_pl" --list "$raw_file" >"$list" || die "member list failed"
  while IFS= read -r index && IFS= read -r type && IFS= read -r link && IFS= read -r name; do
    [ "$index" = "${#mem_name[@]}" ] || die "member index gap"
    mem_name+=("$name")
    mem_type+=("$type")
    mem_link+=("$link")
  done <"$list"
  rm -f -- "$list"
}

read_gnu_names() {
  local list line
  gnu_names=()
  list="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-gnu.XXXXXX")"
  # Same listing as list_members: GNU tar -t, never BusyBox, never -P.
  tar.gnu t -f "$raw_file" >"$list"
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || continue
    gnu_names+=("$line")
  done <"$list"
  rm -f -- "$list"
}

load_members() {
  local -A want=()
  local -A seen=()
  local path i
  mem_name=()
  mem_type=()
  mem_link=()
  mem_wanted=()
  mem_isolated=()
  mem_strip=()
  extract_all=0
  if [ -z "$files_from" ] && [ "${#filtered[@]}" -eq 0 ]; then
    extract_all=1
  fi
  if [ "$extract_all" -eq 0 ]; then
    for path in "${filtered[@]}"; do
      want["$path"]=1
    done
  fi
  read_perl_members
  read_gnu_names
  if [ "${#mem_name[@]}" -ne "${#gnu_names[@]}" ]; then
    die "member list mismatch count gnu=${#gnu_names[@]} parsed=${#mem_name[@]}"
  fi
  for i in "${!mem_name[@]}"; do
    [ "${mem_name[$i]}" = "${gnu_names[$i]}" ] || die "member list mismatch at $i"
    case "${mem_name[$i]}" in
      *$'\t'*) die "member name contains a tab" ;;
      *$'\n'*) die "member name contains a newline" ;;
    esac
    if [ "$extract_all" -eq 1 ] || [ -n "${want[${mem_name[$i]}]:-}" ]; then
      mem_wanted[$i]=1
      seen["${mem_name[$i]}"]=1
    else
      mem_wanted[$i]=0
    fi
  done
  if [ "$extract_all" -eq 0 ]; then
    for path in "${filtered[@]}"; do
      [ -n "${seen[$path]:-}" ] || die "not found in archive: $path"
    done
  fi
  mark_isolated
}

mark_isolated() {
  local -A count=()
  local i stripped
  isolated_any=0
  if [ "${#mem_name[@]}" -eq 0 ]; then
    return 0
  fi
  for i in "${!mem_name[@]}"; do
    stripped="$(strip_unsafe "${mem_name[$i]}")"
    mem_strip[$i]="$stripped"
    count["$stripped"]=$((${count["$stripped"]:-0} + 1))
  done
  for i in "${!mem_name[@]}"; do
    stripped="${mem_strip[$i]}"
    if [ "$stripped" != "${mem_name[$i]}" ] || [ "${count[$stripped]}" -gt 1 ]; then
      mem_isolated[$i]=1
      isolated_any=1
    else
      mem_isolated[$i]=0
    fi
  done
}

# BusyBox drops a hardlink when its target is not in the same extract.
reject_hardlinks() {
  local -A seen=()
  local i run=-1 open=0 key
  if [ "${#mem_name[@]}" -eq 0 ]; then
    return 0
  fi
  for i in "${!mem_name[@]}"; do
    if [ "${mem_wanted[$i]}" -ne 1 ] || [ "${mem_isolated[$i]}" -eq 1 ]; then
      open=0
      if [ "${mem_wanted[$i]}" -eq 1 ] && [ "${mem_type[$i]}" = 1 ]; then
        die "hardlink in per-member extract: ${mem_name[$i]}"
      fi
      continue
    fi
    if [ "$open" -eq 0 ]; then
      run=$((run + 1))
      open=1
    fi
    if [ "${mem_type[$i]}" = 1 ]; then
      key="${run}"$'\n'"${mem_link[$i]}"
      [ -n "${seen[$key]:-}" ] || die "hardlink in per-member extract: ${mem_name[$i]}"
    fi
    key="${run}"$'\n'"${mem_name[$i]}"
    seen["$key"]=1
  done
}

extract_indices() {
  local dest="$1"
  shift
  local idxfile
  [ "$#" -gt 0 ] || return 0
  idxfile="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-idx.XXXXXX")"
  printf '%s\n' "$@" >"$idxfile"
  pack_temp="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-pack.XXXXXX")"
  if ! perl "$member_pl" --emit "$raw_file" "$idxfile" >"$pack_temp"; then
    rm -f -- "$idxfile"
    die "member split failed"
  fi
  rm -f -- "$idxfile"
  mkdir -p -- "$dest"
  busybox tar -xf "$pack_temp" -C "$dest"
  rm -f -- "$pack_temp"
  pack_temp=""
}

place_isolated() {
  local i="$1"
  local name="${mem_name[$i]}"
  local stripped="${mem_strip[$i]}"
  local actual intended
  [ -n "$stripped" ] && [ "$stripped" != "." ] || die "empty member path: $name"
  stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/velnor-tar-m${i}.XXXXXX")"
  mkdir -p -- "$stage_dir/root"
  extract_indices "$stage_dir/root" "$i"
  actual="$stage_dir/root/$stripped"
  intended="$(member_intended "$name")"
  if [ -e "$actual" ] || [ -L "$actual" ]; then
    move_member "$actual" "$intended"
  elif [[ "$name" == */ ]]; then
    mkdir -p -- "$intended"
  else
    die "member missing after extract: $name"
  fi
  rm -rf -- "$stage_dir"
  stage_dir=""
}

extract_planned() {
  local -a batch=()
  local i
  if [ "${#mem_name[@]}" -eq 0 ]; then
    return 0
  fi
  for i in "${!mem_name[@]}"; do
    [ "${mem_wanted[$i]}" -eq 1 ] || continue
    if [ "${mem_isolated[$i]}" -eq 1 ]; then
      if [ "${#batch[@]}" -gt 0 ]; then
        extract_indices "$dest_root" "${batch[@]}"
        batch=()
      fi
      place_isolated "$i"
    else
      batch+=("$i")
    fi
  done
  if [ "${#batch[@]}" -gt 0 ]; then
    extract_indices "$dest_root" "${batch[@]}"
  fi
}

extract_fast() {
  if [ -n "$files_from" ]; then
    run_listed
  else
    run_busybox
  fi
}

extract_absolute() {
  member_tool
  load_members
  if [ "$isolated_any" -eq 0 ]; then
    extract_fast
    return 0
  fi
  reject_hardlinks
  dest_root="${chdir:-$PWD}"
  extract_planned
}

extract_archive() {
  trap cleanup_extract EXIT
  if [ "$absolute" -ne 1 ]; then
    reject_untrusted
    extract_fast
    return 0
  fi
  prepare_raw
  extract_absolute
  if [ -n "$raw_temp" ]; then
    rm -f -- "$raw_temp"
    raw_temp=""
  fi
}
