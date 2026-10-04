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

resource_role_class() {
  case "${MBX_QUALIFICATION_ROLE-}" in
    writer|seed|new-key-writer) printf 'cold\n' ;;
    reader|reader-a|reader-b|corrupt-reader) printf 'hit\n' ;;
    *) return 1 ;;
  esac
}

RESOURCE_SESSION_SCAN_LIMIT=4096
RESOURCE_DEADLINE_STARTED_CS=0
RESOURCE_DEADLINE_TOTAL_CS=0
RESOURCE_DEADLINE_ACTIVE_CS=0
RESOURCE_DEADLINE_PHASE_STARTED_CS=0
RESOURCE_DEADLINE_COMMAND_EXPIRED=0
resource_monotonic_centiseconds() {
  local uptime whole hundredths
  IFS=' ' read -r uptime _ < /proc/uptime || return 1
  [[ "$uptime" =~ ^([0-9]+)\.([0-9]{2})$ ]] || return 1
  whole="${BASH_REMATCH[1]}"
  hundredths="${BASH_REMATCH[2]}"
  printf '%s\n' "$((10#$whole * 100 + 10#$hundredths))"
}
resource_deadline_begin_budget() {
  local seconds="$1" now
  [[ "$seconds" =~ ^[1-9][0-9]{0,2}$ ]] || return 1
  (( 10#$seconds <= 120 )) || return 1
  now="$(resource_monotonic_centiseconds)" || return 1
  RESOURCE_DEADLINE_STARTED_CS="$now"
  RESOURCE_DEADLINE_TOTAL_CS="$((now + 10#$seconds * 100))"
  RESOURCE_DEADLINE_ACTIVE_CS="$RESOURCE_DEADLINE_TOTAL_CS"
  RESOURCE_DEADLINE_PHASE_STARTED_CS="$now"
}

resource_deadline_start_phase() {
  local seconds="$1" now phase_deadline
  [[ "$seconds" =~ ^[1-9][0-9]{0,2}$ ]] || return 1
  (( 10#$seconds <= 120 )) || return 1
  now="$(resource_monotonic_centiseconds)" || return 1
  phase_deadline="$((now + 10#$seconds * 100))"
  (( phase_deadline <= RESOURCE_DEADLINE_TOTAL_CS )) ||
    phase_deadline="$RESOURCE_DEADLINE_TOTAL_CS"
  (( phase_deadline > now )) || return 1
  RESOURCE_DEADLINE_ACTIVE_CS="$phase_deadline"
  RESOURCE_DEADLINE_PHASE_STARTED_CS="$now"
  RESOURCE_DEADLINE_COMMAND_EXPIRED=0
}

resource_deadline_remaining_cs() {
  local now remaining
  (( RESOURCE_DEADLINE_ACTIVE_CS > 0 )) || return 2
  now="$(resource_monotonic_centiseconds)" || return 2
  remaining="$((RESOURCE_DEADLINE_ACTIVE_CS - now))"
  (( remaining > 0 )) || return 1
  printf '%s\n' "$remaining"
}

resource_deadline_elapsed_cs() {
  local started="$1" now
  [[ "$started" =~ ^[0-9]+$ ]] || return 1
  now="$(resource_monotonic_centiseconds)" || return 1
  (( now >= started )) || return 1
  printf '%s\n' "$((now - started))"
}

resource_deadline_command() {
  local remaining seconds command_budget status
  remaining="$(resource_deadline_remaining_cs)" || return 124
  command_budget="$((remaining - 25))"
  if (( command_budget <= 0 )); then RESOURCE_DEADLINE_COMMAND_EXPIRED=1; return 124; fi
  seconds="$(printf '%d.%02d' "$((command_budget / 100))" "$((command_budget % 100))")"
  timeout --signal=KILL "${seconds}s" "$@"
  status=$?
  if (( status == 124 || status == 137 )); then RESOURCE_DEADLINE_COMMAND_EXPIRED=1; fi
  resource_deadline_remaining_cs >/dev/null || { RESOURCE_DEADLINE_COMMAND_EXPIRED=1; return 124; }
  return "$status"
}

resource_deadline_capture_failed() {
  (( $1 == 124 || $1 == 137 )) && RESOURCE_DEADLINE_COMMAND_EXPIRED=1
  return 1
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
  resource_deadline_remaining_cs >/dev/null || return 1
  IFS= read -r row < "/proc/$1/stat" || return 1
  remainder="${row##*) }"
  read -r -a fields <<< "$remainder"
  ((${#fields[@]} > 19)) || return 1
  resource_deadline_remaining_cs >/dev/null || return 1
  printf '%s\t%s\t%s\t%s\n' "${fields[2]}" "${fields[3]}" "${fields[19]}" "${fields[0]}"
}
resource_proc_uids() {
  local line real effective saved filesystem
  resource_deadline_remaining_cs >/dev/null || return 1
  while IFS= read -r line; do
    [[ "$line" == Uid:* ]] || continue
    read -r real effective saved filesystem <<< "${line#Uid:}"
    [[ "$real" =~ ^[0-9]+$ && "$effective" =~ ^[0-9]+$ &&
      "$saved" =~ ^[0-9]+$ && "$filesystem" =~ ^[0-9]+$ ]] || return 1
    resource_deadline_remaining_cs >/dev/null || return 1
    printf '%s\t%s\t%s\t%s\n' "$real" "$effective" "$saved" "$filesystem"
    return 0
  done < "/proc/$1/status" 2>/dev/null
  return 1
}

collect_owned_session_members() {
  local rows process_uid pid pgid sid extra identity actual_pgid actual_sid ticks state
  local uids real_uid effective_uid saved_uid filesystem_uid row_count=0
  owned_session_pids=()
  owned_session_pgids=()
  owned_session_ticks=()
  rows="$(resource_deadline_command bash -o pipefail -c \
    'ps -eo uid=,pid=,pgid=,sid= | head -n "$1"' \
    resource-session-scan "$((RESOURCE_SESSION_SCAN_LIMIT + 1))")" || { resource_deadline_capture_failed "$?"; return 1; }
  while read -r process_uid pid pgid sid extra; do
    [[ -n "${process_uid-}" ]] || continue
    row_count=$((row_count + 1))
    (( row_count <= RESOURCE_SESSION_SCAN_LIMIT )) || return 1
    resource_deadline_remaining_cs >/dev/null || return 1
    [[ -z "${extra-}" ]] || return 1
    [[ "$sid" == "$sampler_sid" ]] || continue
    [[ "$process_uid" =~ ^[0-9]+$ ]] || return 1
    valid_session_leader_pid "$pid" && valid_session_leader_pid "$pgid" &&
      valid_session_leader_pid "$sid" || return 1
    [[ "$pid" != "$$" && "$pid" != "$PPID" ]] || return 1
    if ! identity="$(resource_proc_identity "$pid" 2>/dev/null)"; then
      resource_deadline_remaining_cs >/dev/null || return 1
      [[ ! -e "/proc/$pid/stat" ]] && continue
      return 1
    fi
    IFS=$'\t' read -r actual_pgid actual_sid ticks state <<< "$identity"
    [[ "$actual_pgid" == "$pgid" && "$actual_sid" == "$sid" ]] || return 1
    if ! uids="$(resource_proc_uids "$pid" 2>/dev/null)"; then
      resource_deadline_remaining_cs >/dev/null || return 1
      [[ ! -e "/proc/$pid" ]] && continue
      return 1
    fi
    IFS=$'\t' read -r real_uid effective_uid saved_uid filesystem_uid <<< "$uids"
    [[ "$process_uid" == "$effective_uid" && "$real_uid" == "$sampler_uid" &&
      "$effective_uid" == "$sampler_uid" && "$saved_uid" == "$sampler_uid" &&
      "$filesystem_uid" == "$sampler_uid" ]] || return 1
    [[ "$pid" != "$sampler_pid" || "$ticks" == "$sampler_start_ticks" ]] || return 1
    owned_session_pids+=("$pid")
    owned_session_pgids+=("$pgid")
    owned_session_ticks+=("$ticks")
  done <<< "$rows"
}

owned_session_member_count() {
  collect_owned_session_members || return 1
  owned_session_count_value="${#owned_session_pids[@]}"
}
owned_session_leader_matches() {
  local identity pgid sid ticks state uids real_uid effective_uid saved_uid filesystem_uid args
  identity="$(resource_proc_identity "$sampler_pid" 2>/dev/null)" || return 1
  IFS=$'\t' read -r pgid sid ticks state <<< "$identity"
  [[ "$pgid" == "$sampler_pgid" && "$sid" == "$sampler_sid" &&
    "$ticks" == "$sampler_start_ticks" ]] || return 1
  uids="$(resource_proc_uids "$sampler_pid" 2>/dev/null)" || return 1
  IFS=$'\t' read -r real_uid effective_uid saved_uid filesystem_uid <<< "$uids"
  [[ "$real_uid" == "$sampler_uid" && "$effective_uid" == "$sampler_uid" &&
    "$saved_uid" == "$sampler_uid" && "$filesystem_uid" == "$sampler_uid" ]] || return 1
  args="$(resource_deadline_command ps -p "$sampler_pid" -o args= 2>/dev/null)" || { resource_deadline_capture_failed "$?"; return 1; }
  [[ "$args" == *"$evidence/sampler.sh"* ]]
}

owned_session_member_matches() {
  local pid="$1" expected_pgid="$2" expected_ticks="$3"
  local identity actual_pgid actual_sid actual_ticks state uids real_uid effective_uid saved_uid filesystem_uid
  resource_deadline_remaining_cs >/dev/null || return 1
  [[ "$pid" != "$$" && "$pid" != "$PPID" ]] || return 1
  [[ -r "/proc/$pid/stat" ]] || return 2
  identity="$(resource_proc_identity "$pid" 2>/dev/null)" || {
    [[ ! -e "/proc/$pid/stat" ]] && return 2
    return 1
  }
  IFS=$'\t' read -r actual_pgid actual_sid actual_ticks state <<< "$identity"
  [[ "$actual_pgid" == "$expected_pgid" && "$actual_sid" == "$sampler_sid" &&
    "$actual_ticks" == "$expected_ticks" ]] || return 1
  uids="$(resource_proc_uids "$pid" 2>/dev/null)" || {
    resource_deadline_remaining_cs >/dev/null || return 1
    [[ ! -e "/proc/$pid" ]] && return 2
    return 1
  }
  IFS=$'\t' read -r real_uid effective_uid saved_uid filesystem_uid <<< "$uids"
  [[ "$real_uid" == "$sampler_uid" && "$effective_uid" == "$sampler_uid" &&
    "$saved_uid" == "$sampler_uid" && "$filesystem_uid" == "$sampler_uid" ]]
}

signal_owned_session_members() {
  local signal="$1" index pid pgid ticks status
  collect_owned_session_members || return 1
  for index in "${!owned_session_pids[@]}"; do
    resource_deadline_remaining_cs >/dev/null || return 1
    pid="${owned_session_pids[$index]}"
    pgid="${owned_session_pgids[$index]}"
    ticks="${owned_session_ticks[$index]}"
    if owned_session_member_matches "$pid" "$pgid" "$ticks"; then
      resource_deadline_remaining_cs >/dev/null || return 1
      kill "-$signal" -- "$pid" 2>/dev/null || {
        [[ ! -e "/proc/$pid/stat" ]] || return 1
      }
    else
      status=$?
      (( status == 2 )) || return 1
    fi
    resource_deadline_remaining_cs >/dev/null || return 1
  done
}

wait_for_owned_session_until_deadline() {
  local members empty_passes=0
  while :; do
    resource_deadline_remaining_cs >/dev/null || return 1
    owned_session_member_count || return 2
    members="$owned_session_count_value"
    if [[ "$members" == 0 ]]; then
      empty_passes=$((empty_passes + 1))
      (( empty_passes >= 2 )) && {
        resource_deadline_remaining_cs >/dev/null || return 1
        return 0
      }
    else
      empty_passes=0
    fi
    resource_deadline_command sleep 0.05 || return 1
  done
}

resource_deadline_failure_status() {
  local phase="$1" failure_status
  if (( RESOURCE_DEADLINE_COMMAND_EXPIRED == 1 )); then printf 'expired:%s\n' "$phase"; return; fi
  if resource_deadline_remaining_cs >/dev/null; then
    printf 'failed:%s\n' "$phase"
  else
    failure_status=$?
    if (( failure_status == 1 )); then
      printf 'expired:%s\n' "$phase"
    else
      printf 'clock_error:%s\n' "$phase"
    fi
  fi
}

resource_shutdown_owned_session() {
  local graceful_seconds="$1" term_seconds="$2" kill_seconds="$3"
  local total_seconds phase_start members term_clean=0
  RESOURCE_SHUTDOWN_BUDGET_SECONDS=0
  RESOURCE_SHUTDOWN_ELAPSED_CS=unknown; RESOURCE_SHUTDOWN_GRACEFUL_ELAPSED_CS=0
  RESOURCE_SHUTDOWN_TERM_ELAPSED_CS=0; RESOURCE_SHUTDOWN_KILL_ELAPSED_CS=0
  RESOURCE_SHUTDOWN_STATUS=not_started; RESOURCE_SHUTDOWN_VERIFIED=0
  RESOURCE_SHUTDOWN_REMAINING=unknown
  [[ "$graceful_seconds" =~ ^[1-9][0-9]{0,2}$ &&
    "$term_seconds" =~ ^[1-9][0-9]{0,2}$ && "$kill_seconds" =~ ^[1-9][0-9]{0,2}$ ]] || { RESOURCE_SHUTDOWN_STATUS=invalid_budget; return 1; }
  total_seconds="$((10#$graceful_seconds + 10#$term_seconds + 10#$kill_seconds))"
  RESOURCE_SHUTDOWN_BUDGET_SECONDS="$total_seconds"
  if ! resource_deadline_begin_budget "$total_seconds"; then
    RESOURCE_SHUTDOWN_STATUS=clock_or_budget_error
    return 1
  fi
  if ! resource_deadline_start_phase "$graceful_seconds"; then
    RESOURCE_SHUTDOWN_STATUS=expired:graceful
    return 1
  fi
  phase_start="$RESOURCE_DEADLINE_PHASE_STARTED_CS"
  if ! owned_session_leader_matches; then
    if (( RESOURCE_DEADLINE_COMMAND_EXPIRED == 1 )); then
      RESOURCE_SHUTDOWN_STATUS="$(resource_deadline_failure_status graceful_leader)"
    else
      RESOURCE_SHUTDOWN_STATUS=invalid_leader
    fi
    RESOURCE_SHUTDOWN_GRACEFUL_ELAPSED_CS="$(resource_deadline_elapsed_cs "$phase_start" 2>/dev/null || echo unknown)"
    RESOURCE_SHUTDOWN_ELAPSED_CS="$(resource_deadline_elapsed_cs "$RESOURCE_DEADLINE_STARTED_CS" 2>/dev/null || echo unknown)"
    return 1
  fi
  resource_deadline_remaining_cs >/dev/null || {
    RESOURCE_SHUTDOWN_STATUS="$(resource_deadline_failure_status graceful_leader)"
    return 1
  }
  members=unknown
  if owned_session_member_count 2>/dev/null; then members="$owned_session_count_value"; fi
  if [[ ! "$members" =~ ^[1-9][0-9]*$ ]] ||
    [[ -e "$evidence/sampler.stop" || -L "$evidence/sampler.stop" ]] ||
    ! (set -o noclobber; : > "$evidence/sampler.stop"); then
    RESOURCE_SHUTDOWN_STATUS="$(resource_deadline_failure_status graceful_scan)"
    RESOURCE_SHUTDOWN_GRACEFUL_ELAPSED_CS="$(resource_deadline_elapsed_cs "$phase_start" 2>/dev/null || echo unknown)"
    RESOURCE_SHUTDOWN_ELAPSED_CS="$(resource_deadline_elapsed_cs "$RESOURCE_DEADLINE_STARTED_CS" 2>/dev/null || echo unknown)"
    return 1
  fi
  RESOURCE_SHUTDOWN_VERIFIED=1
  if wait_for_owned_session_until_deadline; then
    RESOURCE_SHUTDOWN_REMAINING=0
    RESOURCE_SHUTDOWN_GRACEFUL_ELAPSED_CS="$(resource_deadline_elapsed_cs "$phase_start" 2>/dev/null || echo unknown)"
    RESOURCE_SHUTDOWN_ELAPSED_CS="$(resource_deadline_elapsed_cs "$RESOURCE_DEADLINE_STARTED_CS" 2>/dev/null || echo unknown)"
    if [[ "$RESOURCE_SHUTDOWN_GRACEFUL_ELAPSED_CS" =~ ^[0-9]+$ &&
      "$RESOURCE_SHUTDOWN_ELAPSED_CS" =~ ^[0-9]+$ ]] &&
      (( RESOURCE_SHUTDOWN_GRACEFUL_ELAPSED_CS <= 10#$graceful_seconds * 100 &&
        RESOURCE_SHUTDOWN_ELAPSED_CS <= total_seconds * 100 )); then
      RESOURCE_SHUTDOWN_STATUS=within_budget
      return 0
    fi
    RESOURCE_SHUTDOWN_STATUS=expired:graceful_record
    return 1
  fi
  RESOURCE_SHUTDOWN_STATUS="$(resource_deadline_failure_status graceful)"
  RESOURCE_SHUTDOWN_GRACEFUL_ELAPSED_CS="$(resource_deadline_elapsed_cs "$phase_start" 2>/dev/null || echo unknown)"
  if resource_deadline_start_phase "$term_seconds"; then
    phase_start="$RESOURCE_DEADLINE_PHASE_STARTED_CS"
    if signal_owned_session_members TERM && wait_for_owned_session_until_deadline; then
      term_clean=1
      RESOURCE_SHUTDOWN_REMAINING=0
    fi
    RESOURCE_SHUTDOWN_TERM_ELAPSED_CS="$(resource_deadline_elapsed_cs "$phase_start" 2>/dev/null || echo unknown)"
  fi
  if (( term_clean == 0 )) && resource_deadline_start_phase "$kill_seconds"; then
    phase_start="$RESOURCE_DEADLINE_PHASE_STARTED_CS"
    if signal_owned_session_members KILL && wait_for_owned_session_until_deadline; then
      RESOURCE_SHUTDOWN_REMAINING=0
    fi
    RESOURCE_SHUTDOWN_KILL_ELAPSED_CS="$(resource_deadline_elapsed_cs "$phase_start" 2>/dev/null || echo unknown)"
  fi
  RESOURCE_SHUTDOWN_ELAPSED_CS="$(resource_deadline_elapsed_cs "$RESOURCE_DEADLINE_STARTED_CS" 2>/dev/null || echo unknown)"
  return 1
}
