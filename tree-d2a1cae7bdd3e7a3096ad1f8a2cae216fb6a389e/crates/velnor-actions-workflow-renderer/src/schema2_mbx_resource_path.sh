escape_path() {
  local value="$1"
  value="${value//%/%25}"
  value="${value//$'\t'/%09}"
  value="${value//$'\n'/%0A}"
  value="${value//$'\r'/%0D}"
  printf '%s' "$value"
}

evidence_marker() {
  printf 'mbx-cache-evidence-v1\t%s\t%s\t%s\n' \
    "${GITHUB_RUN_ID-}" "${GITHUB_RUN_ATTEMPT-}" "${MBX_QUALIFICATION_JOB_ID-}"
}

valid_session_leader_pid() {
  [[ "$1" =~ ^[1-9][0-9]*$ && "$1" != 1 ]]
}

capture_walk_ancestry() {
  local root="$1" current=/ segment
  local -a components=()
  walk_ancestor_paths=(/)
  walk_ancestor_ids=("$(stat -c '%d:%i' /)")
  IFS='/' read -r -a components <<< "${root#/}"
  for segment in "${components[@]}"; do
    [[ -n "$segment" && "$segment" != . && "$segment" != .. ]] || return 1
    current="${current%/}/$segment"
    [[ -d "$current" && ! -L "$current" && "$(realpath -e -- "$current")" == "$current" ]] || return 1
    walk_ancestor_paths+=("$current")
    walk_ancestor_ids+=("$(stat -c '%d:%i' -- "$current")")
  done
  [[ "$current" == "$root" ]]
}

validate_walk_ancestry() {
  local index path
  for index in "${!walk_ancestor_paths[@]}"; do
    path="${walk_ancestor_paths[$index]}"
    [[ -d "$path" && ! -L "$path" && "$(realpath -e -- "$path")" == "$path" ]] || return 1
    [[ "$(stat -c '%d:%i' -- "$path")" == "${walk_ancestor_ids[$index]}" ]] || return 1
  done
}

validate_evidence() {
  local canonical marker mode_bits owner path_stat
  [[ -d "$evidence" && ! -L "$evidence" ]] || return 1
  canonical="$(realpath -e -- "$evidence")" || return 1
  [[ "$canonical" == "$evidence" ]] || return 1
  owner="$(stat -c '%u:%g' -- "$evidence")" || return 1
  [[ "$owner" == "$(id -u):$(id -g)" ]] || return 1
  capture_walk_ancestry "$evidence" || return 1
  validate_walk_ancestry || return 1
  [[ -f "$evidence/private.marker" && ! -L "$evidence/private.marker" ]] || return 1
  [[ "$(stat -c '%h:%a:%u:%g' -- "$evidence/private.marker")" == "1:600:$(id -u):$(id -g)" ]] || return 1
  marker="$(cat -- "$evidence/private.marker")"
  [[ "$marker" == "$(evidence_marker)" ]] || return 1
  [[ -f "$evidence/private.identity" && ! -L "$evidence/private.identity" ]] || return 1
  [[ "$(stat -c '%h:%a:%u:%g' -- "$evidence/private.identity")" == "1:600:$(id -u):$(id -g)" ]] || return 1
  [[ "$(cat -- "$evidence/private.identity")" == "$(stat -c '%d:%i:%u:%g' -- "$evidence")" ]] || return 1
  mode_bits="$(stat -c '%a' -- "$evidence")"
  [[ "$mode_bits" == 700 ]] || return 1
  [[ -f "$evidence/root-registry.tsv" && ! -L "$evidence/root-registry.tsv" ]] || return 1
  [[ "$(stat -c '%h:%a:%u:%g' -- "$evidence/root-registry.tsv")" == "1:600:$(id -u):$(id -g)" ]] || return 1
  [[ -f "$evidence/sampler.sh" && ! -L "$evidence/sampler.sh" ]] || return 1
  [[ "$(stat -c '%h:%a:%u:%g' -- "$evidence/sampler.sh")" == "1:700:$(id -u):$(id -g)" ]] || return 1
  [[ -f "$evidence/path-validation.sh" && ! -L "$evidence/path-validation.sh" ]] || return 1
  [[ "$(stat -c '%h:%a:%u:%g' -- "$evidence/path-validation.sh")" == "1:700:$(id -u):$(id -g)" ]] || return 1
  [[ "$MBX_QUALIFICATION_PHASE_FILE" == "$evidence/phases.tsv" ]] || return 1
  [[ -f "$evidence/phases.tsv" && ! -L "$evidence/phases.tsv" ]] || return 1
  path_stat="$(stat -c '%h:%a:%u:%g' -- "$evidence/phases.tsv")"
  [[ "$path_stat" == "1:600:$(id -u):$(id -g)" ]]
}

resource_proc_identity() {
  local row remainder
  local -a fields=()
  IFS= read -r row < "/proc/$1/stat" || return 1
  remainder="${row##*) }"
  read -r -a fields <<< "$remainder"
  ((${#fields[@]} > 19)) || return 1
  printf '%s\t%s\t%s\t%s\n' "${fields[2]}" "${fields[3]}" "${fields[19]}" "${fields[0]}"
}

collect_owned_session_members() {
  local rows process_uid pid pgid sid proc_uid identity actual_pgid actual_sid ticks state
  owned_session_pids=()
  owned_session_pgids=()
  owned_session_ticks=()
  rows="$(ps -eo uid=,pid=,pgid=,sid=)" || return 1
  while read -r process_uid pid pgid sid; do
    [[ "$sid" == "$sampler_sid" ]] || continue
    [[ "$process_uid" =~ ^[0-9]+$ && "$pid" =~ ^[0-9]+$ && "$pgid" =~ ^[0-9]+$ ]] || return 1
    [[ "$pid" != "$$" && "$pid" != "$PPID" ]] || return 1
    if ! identity="$(resource_proc_identity "$pid" 2>/dev/null)"; then
      [[ ! -e "/proc/$pid/stat" ]] && continue
      return 1
    fi
    IFS=$'\t' read -r actual_pgid actual_sid ticks state <<< "$identity"
    [[ "$actual_pgid" == "$pgid" && "$actual_sid" == "$sid" &&
      "$process_uid" == "$sampler_uid" ]] || return 1
    if ! proc_uid="$(stat -c '%u' -- "/proc/$pid" 2>/dev/null)"; then
      [[ ! -e "/proc/$pid" ]] && continue
      return 1
    fi
    [[ "$proc_uid" == "$sampler_uid" ]] || return 1
    [[ "$pid" != "$sampler_pid" || "$ticks" == "$sampler_start_ticks" ]] || return 1
    owned_session_pids+=("$pid")
    owned_session_pgids+=("$pgid")
    owned_session_ticks+=("$ticks")
  done <<< "$rows"
}

owned_session_member_count() {
  collect_owned_session_members || return 1
  printf '%s\n' "${#owned_session_pids[@]}"
}

owned_session_leader_matches() {
  local identity pgid sid ticks state owner args
  identity="$(resource_proc_identity "$sampler_pid" 2>/dev/null)" || return 1
  IFS=$'\t' read -r pgid sid ticks state <<< "$identity"
  [[ "$pgid" == "$sampler_pgid" && "$sid" == "$sampler_sid" &&
    "$ticks" == "$sampler_start_ticks" ]] || return 1
  owner="$(stat -c '%u' -- "/proc/$sampler_pid" 2>/dev/null)" || return 1
  [[ "$owner" == "$sampler_uid" ]] || return 1
  args="$(ps -p "$sampler_pid" -o args= 2>/dev/null)" || return 1
  [[ "$args" == *"$evidence/sampler.sh"* ]]
}

owned_session_member_matches() {
  local pid="$1" expected_pgid="$2" expected_ticks="$3"
  local identity actual_pgid actual_sid actual_ticks state proc_uid
  [[ "$pid" != "$$" && "$pid" != "$PPID" ]] || return 1
  [[ -r "/proc/$pid/stat" ]] || return 2
  identity="$(resource_proc_identity "$pid" 2>/dev/null)" || {
    [[ ! -e "/proc/$pid/stat" ]] && return 2
    return 1
  }
  IFS=$'\t' read -r actual_pgid actual_sid actual_ticks state <<< "$identity"
  [[ "$actual_pgid" == "$expected_pgid" && "$actual_sid" == "$sampler_sid" &&
    "$actual_ticks" == "$expected_ticks" ]] || return 1
  proc_uid="$(stat -c '%u' -- "/proc/$pid" 2>/dev/null)" || {
    [[ ! -e "/proc/$pid" ]] && return 2
    return 1
  }
  [[ "$proc_uid" == "$sampler_uid" ]]
}

signal_owned_session_members() {
  local signal="$1" index pid pgid ticks status
  collect_owned_session_members || return 1
  for index in "${!owned_session_pids[@]}"; do
    pid="${owned_session_pids[$index]}"
    pgid="${owned_session_pgids[$index]}"
    ticks="${owned_session_ticks[$index]}"
    if owned_session_member_matches "$pid" "$pgid" "$ticks"; then
      kill "-$signal" -- "$pid" 2>/dev/null || {
        [[ ! -e "/proc/$pid/stat" ]] || return 1
      }
    else
      status=$?
      (( status == 2 )) || return 1
    fi
  done
}

wait_for_owned_session() {
  local limit="$1" count=0 members
  while (( count < limit )); do
    members="$(owned_session_member_count)" || return 2
    [[ "$members" == 0 ]] && return 0
    sleep 1
    count=$((count + 1))
  done
  members="$(owned_session_member_count)" || return 2
  [[ "$members" == 0 ]]
}
