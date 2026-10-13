# Sourced by tar-absolute.sh. -P members that BusyBox would strip share one
# path, so each of those members is extracted into an index-keyed directory
# and moved alone. A hardlink needs its target in the same BusyBox extract;
# per-member staging cannot keep that inode.
# The decompressor is a stream. One fifo at a time reaches BusyBox, then it
# is removed. /tmp never holds the uncompressed archive.

stage_dir=""
seq_dir=""
plan_file=""
prod_pid=""
bb_pid=""
dest_root=""
isolated_any=0
extract_all=0
member_pl=""

cleanup_extract() {
  if [ -n "${bb_pid}" ]; then
    kill "${bb_pid}" 2>/dev/null || true
    wait "${bb_pid}" 2>/dev/null || true
    bb_pid=""
  fi
  if [ -n "${seq_dir}" ]; then
    : >"${seq_dir}/done" 2>/dev/null || true
  fi
  if [ -n "${prod_pid}" ]; then
    local child kids
    kids="$(ps -o pid= --ppid "${prod_pid}" 2>/dev/null || true)"
    for child in $kids; do
      kill "${child}" 2>/dev/null || true
    done
    kill "${prod_pid}" 2>/dev/null || true
    wait "${prod_pid}" 2>/dev/null || true
    prod_pid=""
  fi
  if [ -n "${stage_dir}" ]; then
    rm -rf -- "$stage_dir"
    stage_dir=""
  fi
  if [ -n "${seq_dir}" ]; then
    rm -rf -- "$seq_dir"
    seq_dir=""
  fi
  if [ -n "${plan_file}" ]; then
    rm -f -- "$plan_file"
    plan_file=""
  fi
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
  if ! stream_archive | perl "$member_pl" --list >"$list"; then
    rm -f -- "$list"
    die "member list failed"
  fi
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
  if ! list_members "$list"; then
    rm -f -- "$list"
    die "member list failed"
  fi
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

# kill -0 is true for a zombie, so the state byte is what ends the poll.
child_state() {
  local pid="$1"
  local rest
  [ -r "/proc/$pid/stat" ] || return 1
  rest="$(sed -n 's/.*) //p' "/proc/$pid/stat")"
  printf '%s' "${rest%% *}"
}

# Producer death while BusyBox is blocked on a fifo is a hang, not a slow extract.
wait_child() {
  local child="$1"
  local prod="$2"
  local state
  while true; do
    state="$(child_state "$child" || true)"
    if [ -z "$state" ] || [ "$state" = Z ]; then
      wait "$child"
      return
    fi
    if ! kill -0 "$prod" 2>/dev/null; then
      sleep 0.2
      state="$(child_state "$child" || true)"
      if [ -n "$state" ] && [ "$state" != Z ]; then
        kill "$child" 2>/dev/null || true
        wait "$child" 2>/dev/null || true
        return 1
      fi
      wait "$child"
      return
    fi
    sleep 0.05
  done
}

install_member() {
  local root="$1"
  local i="$2"
  local name="${mem_name[$i]}"
  local stripped="${mem_strip[$i]}"
  local actual intended
  [ -n "$stripped" ] && [ "$stripped" != "." ] || die "empty member path: $name"
  actual="$root/$stripped"
  intended="$(member_intended "$name")"
  if [ -e "$actual" ] || [ -L "$actual" ]; then
    move_member "$actual" "$intended"
  elif [[ "$name" == */ ]]; then
    mkdir -p -- "$intended"
  else
    die "member missing after extract: $name"
  fi
}

extract_root_fifo() {
  local fifo="$1"
  mkdir -p -- "$dest_root"
  busybox tar -xf "$fifo" -C "$dest_root" &
  bb_pid=$!
  wait_child "$bb_pid" "$prod_pid" || die "member extract failed"
  bb_pid=""
}

extract_one_fifo() {
  local fifo="$1"
  local index="$2"
  stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/velnor-tar-m${index}.XXXXXX")"
  mkdir -p -- "$stage_dir/root"
  busybox tar -xf "$fifo" -C "$stage_dir/root" &
  bb_pid=$!
  wait_child "$bb_pid" "$prod_pid" || die "member extract failed"
  bb_pid=""
  install_member "$stage_dir/root" "$index"
  rm -rf -- "$stage_dir"
  stage_dir=""
}

# One decompress for every wanted member. Isolated members are their own
# group so a colliding stripped path cannot share a BusyBox extract.
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

extract_planned() {
  local -a batch=()
  local -a group_kind=()
  local i
  [ "${#mem_name[@]}" -gt 0 ] || return 0
  plan_file="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-plan.XXXXXX")"
  for i in "${!mem_name[@]}"; do
    [ "${mem_wanted[$i]}" -eq 1 ] || continue
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
  extract_absolute
}
