# Sourced by tar-shim.sh. BusyBox strips leading "/" and ".." and rejects -P.
# GNU tar cannot stat or open those paths under qemu-user (openat2 ENOSYS).
# -P extract: stage each stripped member by archive index, then move that
# inode to the path GNU -P would have opened. -P create uses velnor-tar-pax.

strip_unsafe() {
  local s="$1"
  local output_var="${2:-}"
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
  if [ -n "$output_var" ]; then
    printf -v "$output_var" '%s' "$s"
  else
    printf '%s' "$s"
  fi
}

norm_path() {
  local input="$1"
  local output_var="${2:-}"
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
  local result="" p
  if [ "$abs" -eq 1 ]; then
    if [ "${#stack[@]}" -eq 0 ]; then
      result="/"
    else
      for p in "${stack[@]}"; do
        result="$result/$p"
      done
    fi
  elif [ "${#stack[@]}" -eq 0 ]; then
    result="."
  else
    local IFS=/
    result="${stack[*]}"
  fi
  if [ -n "$output_var" ]; then
    printf -v "$output_var" '%s' "$result"
  else
    printf '%s' "$result"
  fi
}

member_intended() {
  local base output_var="${2:-}"
  if [[ "$1" == /* ]]; then
    norm_path "$1" "$output_var"
    return
  fi
  base="${chdir:-$PWD}"
  norm_path "$base/$1" "$output_var"
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
  local raw index type link name mode
  # qemu-user returns ENOSYS for GNU tar openat2/statx. The perl walker
  # reads headers only and prints the stored name. BusyBox tar -t rewrites
  # "../", so it is not the list source.
  raw="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-list.XXXXXX")"
  if ! stream_archive | perl "${_velnor_tar_here}/tar-member.pl" --list >"$raw"; then
    rm -f -- "$raw"
    return 1
  fi
  : >"$out"
  while IFS= read -r index && IFS= read -r type && IFS= read -r link && \
    IFS= read -r name && IFS= read -r mode; do
    printf '%s\n' "$name" >>"$out" || {
      rm -f -- "$raw"
      return 1
    }
  done <"$raw"
  rm -f -- "$raw"
}

move_member() {
  local actual="$1" intended="$2" parent
  if [ -d "$intended" ] && [ ! -L "$intended" ] && [ ! -d "$actual" ]; then
    die "destination is a directory: $intended"
  fi
  parent="${intended%/*}"
  mkdir -p -- "${parent:-/}"
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

# -P may place a member under the runner work tree, outside the workspace.
# The inode and the resolved target must stay in an allowed root.
job_path() {
  local path="$1"
  local root
  root="$(norm_path "${chdir:-$PWD}")"
  path_under "$root" "$path" && return 0
  path_under /home/runner/work "$path" && return 0
  path_under /home/runner/_work "$path" && return 0
  return 1
}

# A symlink may point only inside an allowed root. A later member must
# not walk through an archive symlink or a symlink already on disk.
reject_symlink_traversal() {
  local -A links=()
  local -A checked_prefixes=()
  local -A checked_member_parents=()
  local i name target dir intended root key prefix parent_name
  root="$(norm_path "${chdir:-$PWD}")"
  for i in "${!mem_name[@]}"; do
    [ "${mem_wanted[$i]}" -eq 1 ] || continue
    [ "${mem_type[$i]}" = 2 ] || continue
    name="${mem_name[$i]}"
    member_intended "$name" key
    job_path "$key" || die "symlink escapes destination: $name"
    links["$key"]=1
    target="${mem_link[$i]}"
    [ -n "$target" ] || die "symlink escapes destination: $name"
    if [[ "$target" == /* ]]; then
      intended="$(norm_path "$target")"
    else
      dir="$(dirname -- "$key")"
      intended="$(norm_path "$dir/$target")"
    fi
    if ! job_path "$intended"; then
      if [ "$absolute" -eq 1 ] && [[ "$target" != /* ]]; then
        continue
      fi
      die "symlink escapes destination: $name"
    fi
  done
  for i in "${!mem_name[@]}"; do
    [ "${mem_wanted[$i]}" -eq 1 ] || continue
    name="${mem_name[$i]}"
    parent_name="$name"
    while [[ "$parent_name" == */ ]]; do
      parent_name="${parent_name%/}"
    done
    if [[ "$parent_name" == */* ]]; then
      parent_name="${parent_name%/*}"
      [ -n "$parent_name" ] || parent_name="/"
    else
      parent_name="."
    fi
    [ -z "${checked_member_parents[$parent_name]:-}" ] || continue
    checked_member_parents["$parent_name"]=1
    member_intended "$parent_name" intended
    prefix="$intended"
    while [ "$prefix" != "/" ] && [ "$prefix" != "." ]; do
      [ -z "${checked_prefixes[$prefix]:-}" ] || break
      checked_prefixes["$prefix"]=1
      if [ -n "${links[$prefix]:-}" ] || [ -L "$prefix" ]; then
        die "member traverses symlink: $name"
      fi
      safe["$prefix"]=1
      [ "$prefix" = "$root" ] && break
      prefix="${prefix%/*}"
      [ -n "$prefix" ] || prefix="/"
    done
  done
  return 0
}

# shellcheck disable=SC1091
. "$_velnor_tar_here/tar-extract.sh"

# A directory member stays put. Moving it drops later children still in the stage.
extract_stage_fifo() {
  local fifo="$1" indices="$2" i intended
  stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/velnor-tar-mstage.XXXXXX")"
  mkdir -p -- "$stage_dir/root"
  busybox tar -xf "$fifo" -C "$stage_dir/root" &
  bb_pid=$!
  wait_child "$bb_pid" "$prod_pid" || die "member extract failed"
  bb_pid=""
  for i in $indices; do
    if [ "${mem_type[$i]}" = 5 ] || [[ "${mem_name[$i]}" == */ ]]; then
      intended="$(member_intended "${mem_name[$i]}")"
      mkdir -p -- "$intended"
    else
      install_member "$stage_dir/root" "$i"
    fi
  done
  rm -rf -- "$stage_dir"
  stage_dir=""
}

extract_planned() {
  local -a batch=()
  local -a stage=()
  local -a group_kind=()
  local -A strip_count=()
  local i stripped
  [ "${#mem_name[@]}" -gt 0 ] || return 0
  for i in "${!mem_name[@]}"; do
    [ "${mem_wanted[$i]}" -eq 1 ] || continue
    stripped="${mem_strip[$i]}"
    strip_count["$stripped"]=$((${strip_count["$stripped"]:-0} + 1))
  done
  plan_file="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-plan.XXXXXX")"
  for i in "${!mem_name[@]}"; do
    [ "${mem_wanted[$i]}" -eq 1 ] || continue
    if [ "${mem_isolated[$i]}" -eq 1 ] && [ "${strip_count[${mem_strip[$i]}]}" -eq 1 ]; then
      if [ "${#batch[@]}" -gt 0 ]; then
        printf '%s\n' "${batch[*]}" >>"$plan_file"
        group_kind+=("root")
        batch=()
      fi
      stage+=("$i")
      continue
    fi
    if [ "${#stage[@]}" -gt 0 ]; then
      printf '%s\n' "${stage[*]}" >>"$plan_file"
      group_kind+=("stage")
      stage=()
    fi
    if [ "${mem_isolated[$i]}" -eq 1 ]; then
      if [ "${#batch[@]}" -gt 0 ]; then
        printf '%s\n' "${batch[*]}" >>"$plan_file"
        group_kind+=("root")
        batch=()
      fi
      printf '%s\n' "$i" >>"$plan_file"
      group_kind+=("one")
      continue
    fi
    batch+=("$i")
  done
  if [ "${#stage[@]}" -gt 0 ]; then
    printf '%s\n' "${stage[*]}" >>"$plan_file"
    group_kind+=("stage")
  fi
  if [ "${#batch[@]}" -gt 0 ]; then
    printf '%s\n' "${batch[*]}" >>"$plan_file"
    group_kind+=("root")
  fi
  if [ "${#group_kind[@]}" -eq 0 ]; then
    rm -f -- "$plan_file"
    plan_file=""
    return 0
  fi
  run_plan "$plan_file" group_kind
  rm -f -- "$plan_file"
  plan_file=""
}

run_plan() {
  local plan="$1"
  local -n kinds=$2
  local seq=0 line fifo
  seq_dir="$(mktemp -d "${TMPDIR:-/tmp}/velnor-tar-seq.XXXXXX")"
  (
    set -o pipefail
    stream_archive | perl "$member_pl" --emit "$plan" "$seq_dir"
  ) &
  prod_pid=$!
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || continue
    fifo="$seq_dir/$seq.fifo"
    mkfifo "$fifo"
    if [ "${kinds[$seq]}" = root ]; then
      extract_root_fifo "$fifo"
    elif [ "${kinds[$seq]}" = stage ]; then
      extract_stage_fifo "$fifo" "$line"
    else
      extract_one_fifo "$fifo" "$line"
    fi
    rm -f -- "$fifo"
    seq=$((seq + 1))
  done <"$plan"
  : >"$seq_dir/done"
  wait "$prod_pid" || die "member split failed"
  prod_pid=""
  rm -rf -- "$seq_dir"
  seq_dir=""
}

finish_tar() {
  local needs_pax=0 path stripped
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
