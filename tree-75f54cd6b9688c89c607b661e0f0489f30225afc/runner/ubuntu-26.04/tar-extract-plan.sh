# Group streamed members that can share a staged BusyBox extraction.

record_directory_metadata() {
  local root="$1"
  shift
  local index actual intended mode mask batch
  mask="$(umask)"
  batch="$(mktemp "${TMPDIR:-/tmp}/velnor-dir-batch.XXXXXX")"
  for index in "$@"; do
    [ "${mem_type[$index]}" = 5 ] || continue
    actual="$root/${mem_strip[$index]}"
    [ -d "$actual" ] && [ ! -L "$actual" ] || continue
    member_intended "${mem_name[$index]}" intended
    printf -v mode '%o' "$(( (0${mem_mode[$index]}) & ~(0${mask}) ))"
    printf '%s\0%s\0%s\0' "$actual" "$intended" "$mode" >>"$batch"
  done
  if [ -s "$batch" ]; then
    [ -n "${dir_meta_file}" ] || dir_meta_file="$(mktemp "${TMPDIR:-/tmp}/velnor-dir-meta.XXXXXX")"
    if ! perl "${_velnor_tar_here}/tar-dir-meta.pl" record "$dir_meta_file" <"$batch"; then
      rm -f -- "$batch"
      die "cannot record directory metadata"
    fi
  fi
  rm -f -- "$batch"
}

restore_directory_metadata() {
  [ -n "${dir_meta_file}" ] && [ -s "${dir_meta_file}" ] || return 0
  perl "${_velnor_tar_here}/tar-dir-meta.pl" restore "$dir_meta_file"
}

extract_root_fifo() {
  local fifo="$1"
  shift
  local -a indices=("$@")
  mkdir -p -- "$dest_root"
  busybox tar -xf "$fifo" -C "$dest_root" &
  bb_pid=$!
  wait_child "$bb_pid" "$prod_pid" || die "member extract failed"
  bb_pid=""
  record_directory_metadata "$dest_root" "${indices[@]}"
}

extract_staged_fifo() {
  local fifo="$1"
  shift
  local index
  local -a indices=("$@")
  stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/velnor-tar-group.XXXXXX")"
  mkdir -p -- "$stage_dir/root"
  busybox tar -xf "$fifo" -C "$stage_dir/root" &
  bb_pid=$!
  wait_child "$bb_pid" "$prod_pid" || die "member extract failed"
  bb_pid=""
  record_directory_metadata "$stage_dir/root" "${indices[@]}"
  for index in "${indices[@]}"; do
    install_member "$stage_dir/root" "$index"
  done
  rm -rf -- "$stage_dir"
  stage_dir=""
}

extract_direct_fifo() {
  local fifo="$1"
  local root="$2"
  shift 2
  local -a indices=("$@")
  mkdir -p -- "$root"
  busybox tar -xf "$fifo" -C "$root" &
  bb_pid=$!
  wait_child "$bb_pid" "$prod_pid" || die "member extract failed"
  bb_pid=""
  record_directory_metadata "$root" "${indices[@]}"
}

# The Perl producer streams selected records through one FIFO per group.
run_plan() {
  local plan="$1"
  local -n kinds=$2
  local -n bases=$3
  local map="$4"
  local seq=0 line fifo
  local -a indices=()
  seq_dir="$(mktemp -d "${TMPDIR:-/tmp}/velnor-tar-seq.XXXXXX")"
  (
    set -o pipefail
    stream_archive | perl "$member_pl" --emit "$plan" "$seq_dir" "$map"
  ) &
  prod_pid=$!
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || continue
    fifo="$seq_dir/$seq.fifo"
    mkfifo "$fifo"
    read -r -a indices <<<"$line"
    if [ "${kinds[$seq]}" = root ]; then
      extract_root_fifo "$fifo" "${indices[@]}"
    elif [ "${kinds[$seq]}" = direct ]; then
      extract_direct_fifo "$fifo" "${bases[$seq]}" "${indices[@]}"
    else
      extract_staged_fifo "$fifo" "${indices[@]}"
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

path_base_for() {
  local intended="$1"
  local relative="$2"
  local output_var="${3:-}" base
  [ -n "$relative" ] && [ "$relative" != "." ] || return 1
  if [ "$intended" = "/$relative" ]; then
    base="/"
  elif [[ "$intended" == *"/$relative" ]]; then
    base="${intended%/$relative}"
    base="${base:-/}"
  else
    return 1
  fi
  if [ -n "$output_var" ]; then
    printf -v "$output_var" '%s' "$base"
  else
    printf '%s' "$base"
  fi
}

flush_isolated_group() {
  local plan="$1"
  local kind="$2"
  local base="$3"
  local -n batch_ref=$4 kind_ref=$5 base_ref=$6
  [ "${#batch_ref[@]}" -gt 0 ] || return 0
  printf '%s\n' "${batch_ref[*]}" >>"$plan"
  kind_ref+=("$kind")
  base_ref+=("$base")
  batch_ref=()
}

flush_root_group() {
  local plan="$1"
  local -n batch_ref=$2 kind_ref=$3 base_ref=$4
  [ "${#batch_ref[@]}" -gt 0 ] || return 0
  printf '%s\n' "${batch_ref[*]}" >>"$plan"
  kind_ref+=("root")
  base_ref+=("")
  batch_ref=()
}

flush_pending_isolated() {
  local plan="$1"
  local -n batch_ref=$2 kind_ref=$3 base_ref=$4 base_value=$5 direct_value=$6
  local kind=staged
  [ "${#batch_ref[@]}" -gt 0 ] || return 0
  [ "$direct_value" -eq 0 ] || kind=direct
  flush_isolated_group "$plan" "$kind" "$base_value" "$2" "$3" "$4"
  base_value=""
  direct_value=0
}

group_path_conflicts() {
  local key="$1"
  local -n seen="$2" descendants="$3"
  local parent="$key"
  [ -z "${seen[.]:-}" ] || return 0
  [ -z "${seen[/]:-}" ] || return 0
  [ "$key" != "." ] || return 0
  [ "$key" != "/" ] || return 0
  [ -z "${seen[$key]:-}" ] || return 0
  [ -z "${descendants[$key]:-}" ] || return 0
  while [[ "$parent" == */* ]]; do
    parent="${parent%/*}"
    [ -n "$parent" ] || parent="/"
    [ -z "${seen[$parent]:-}" ] || return 0
    [ "$parent" != "/" ] || break
  done
  return 1
}

group_path_add() {
  local key="$1"
  local -n seen="$2" descendants="$3"
  local parent="$key"
  seen["$key"]=1
  while [[ "$parent" == */* ]]; do
    parent="${parent%/*}"
    [ -n "$parent" ] || parent="/"
    descendants["$parent"]=$((${descendants["$parent"]:-0} + 1))
    [ "$parent" != "/" ] || break
  done
}

plan_isolated_member() {
  local plan="$1" index="$2" actual intended candidate_base conflict stripped raw_name parent_key
  local -n root_batch_ref=$3 staged_batch_ref=$4 group_kind_ref=$5 group_base_ref=$6 \
    staged_seen_ref=$7 staged_descendants_ref=$8 destination_seen_ref=$9 \
    destination_descendants_ref=${10} staged_base_ref=${11} staged_direct_ref=${12} \
    parent_base_ref=${13}
  if [ "${#root_batch_ref[@]}" -gt 0 ]; then
    flush_root_group "$plan" "$3" "$5" "$6"
  fi
  stripped="${mem_strip[$index]}"
  case "$stripped" in
    "" | . | .. | ./* | ../* | */./* | */. | */../* | */.. | *//* | */ | /*)
      norm_path "$stripped" actual
      ;;
    *) actual="$stripped" ;;
  esac
  [ -n "$actual" ] && [ "$actual" != "." ] || die "empty member path: ${mem_name[$index]}"
  mem_emit_path[$index]="$actual"
  raw_name="${mem_name[$index]}"
  while [[ "$raw_name" == */ ]]; do
    raw_name="${raw_name%/}"
  done
  if [[ "$raw_name" == */* ]]; then
    parent_key="${raw_name%/*}"
    [ -n "$parent_key" ] || parent_key="/"
  else
    parent_key="."
  fi
  if [ -n "${parent_base_ref[$parent_key]:-}" ] && \
    [ "${parent_base_ref[$parent_key]}" != "!" ]; then
    candidate_base="${parent_base_ref[$parent_key]}"
    if [ "$candidate_base" = / ]; then
      intended="/$actual"
    else
      intended="$candidate_base/$actual"
    fi
  else
    member_intended "${mem_name[$index]}" intended
    path_base_for "$intended" "$actual" candidate_base || candidate_base=""
    [ -n "${parent_base_ref[$parent_key]:-}" ] || \
      parent_base_ref["$parent_key"]="${candidate_base:-!}"
  fi
  if [ "${#staged_batch_ref[@]}" -gt 0 ] && [ "$staged_direct_ref" -eq 1 ] && \
    [ "$candidate_base" != "$staged_base_ref" ]; then
    flush_pending_isolated "$plan" "$4" "$5" "$6" "${11}" "${12}"
    staged_seen_ref=()
    staged_descendants_ref=()
    destination_seen_ref=()
    destination_descendants_ref=()
  fi
  conflict=0
  if [ "${#staged_batch_ref[@]}" -gt 0 ]; then
    if group_path_conflicts "$actual" staged_seen_ref staged_descendants_ref; then
      conflict=1
    fi
    if [ "$conflict" -eq 0 ] && [ "$staged_direct_ref" -eq 0 ] && \
      group_path_conflicts "$intended" destination_seen_ref destination_descendants_ref; then
      conflict=1
    fi
  fi
  if [ "$conflict" -eq 1 ]; then
    flush_pending_isolated "$plan" "$4" "$5" "$6" "${11}" "${12}"
    staged_seen_ref=()
    staged_descendants_ref=()
    destination_seen_ref=()
    destination_descendants_ref=()
  fi
  if [ "${#staged_batch_ref[@]}" -eq 0 ]; then
    staged_base_ref="$candidate_base"
    staged_direct_ref=1
    [ -n "$candidate_base" ] || staged_direct_ref=0
  elif [ "$staged_direct_ref" -eq 1 ] && [ "$candidate_base" != "$staged_base_ref" ]; then
    staged_direct_ref=0
  fi
  staged_batch_ref+=("$index")
  group_path_add "$actual" staged_seen_ref staged_descendants_ref
  if [ "$staged_direct_ref" -eq 0 ]; then
    group_path_add "$intended" destination_seen_ref destination_descendants_ref
  fi
}

build_extraction_plan() {
  local i
  local -a root_batch=() staged_batch=() group_kind=() group_base=()
  local staged_base="" staged_direct=0
  local -A staged_seen=() staged_descendants=()
  local -A destination_seen=() destination_descendants=() planned_parent_base=()
  plan_file="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-plan.XXXXXX")"
  for i in "${!mem_name[@]}"; do
    [ "${mem_wanted[$i]}" -eq 1 ] || continue
    if [ "${mem_isolated[$i]}" -eq 1 ]; then
      plan_isolated_member "$plan_file" "$i" root_batch staged_batch group_kind group_base \
        staged_seen staged_descendants destination_seen destination_descendants staged_base staged_direct \
        planned_parent_base
    else
      flush_pending_isolated "$plan_file" staged_batch group_kind group_base staged_base staged_direct
      staged_seen=()
      staged_descendants=()
      destination_seen=()
      destination_descendants=()
      root_batch+=("$i")
    fi
  done
  flush_root_group "$plan_file" root_batch group_kind group_base
  flush_pending_isolated "$plan_file" staged_batch group_kind group_base staged_base staged_direct
  planned_group_kind=("${group_kind[@]}")
  planned_group_base=("${group_base[@]}")
}

extract_planned() {
  local i
  planned_group_kind=()
  planned_group_base=()
  [ "${#mem_name[@]}" -gt 0 ] || return 0
  build_extraction_plan
  if [ "${#planned_group_kind[@]}" -eq 0 ]; then
    rm -f -- "$plan_file"
    plan_file=""
    return 0
  fi
  map_file="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-map.XXXXXX")"
  for i in "${!mem_emit_path[@]}"; do
    printf '%s\t%s\n' "$i" "${mem_emit_path[$i]}" >>"$map_file"
  done
  run_plan "$plan_file" planned_group_kind planned_group_base "$map_file"
  restore_directory_metadata || die "cannot restore directory metadata"
  dir_metadata_restored=1
  rm -f -- "$map_file"
  map_file=""
  rm -f -- "$plan_file"
  plan_file=""
}
