//! Bounded retrieval of exact observer and victim step logs.

pub(super) const RESTORE_LOG_HELPER: &str = r#"
cleanup_transport_root() {
  local root="$1" name limit path
  private_root_open "$root" || return 1
  for name in log-api.headers log-api.status log-api.stderr log-signed.headers \
    restore-step.log log-signed.status log-signed.stderr; do
    path="$root/$name"
    if [ -e "$path" ] || [ -L "$path" ]; then
      limit=65536
      case "$name" in restore-step.log) limit=1048576 ;; esac
      private_file_valid "$root" "$path" "$limit" || return 1
      rm -- "$path" || return 1
    fi
  done
}

valid_runner_timestamp() {
  local timestamp="$1" normalized
  [[ "$timestamp" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{7}Z$ ]] || return 1
  normalized="$(date -u -d "$timestamp" '+%Y-%m-%dT%H:%M:%S.%7NZ' 2>/dev/null || printf invalid)"
  [ "$normalized" = "$timestamp" ]
}

decimal_less() {
  LC_ALL=C awk -v left="$1" -v right="$2" 'BEGIN {
    if (length(left) < length(right)) exit 0
    if (length(left) > length(right)) exit 1
    if (("x" left) < ("x" right)) exit 0
    exit 1
  }'
}

save_log_proves_partial_before_cancel_error() {
  local path="$1" line timestamp body sent total partial=false
  private_file_valid "$SAVE_LOG_ROOT" "$path" 1048576 || return 1
  while IFS= read -r line || [ -n "$line" ]; do
    if [[ "$line" =~ ^([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{7}Z)\ (.*)$ ]]; then
      timestamp="${BASH_REMATCH[1]}"
      body="${BASH_REMATCH[2]}"
      if [ "$body" = '##[error]The operation was canceled.' ]; then
        valid_runner_timestamp "$timestamp" || return 1
        [ "$partial" = true ] || return 1
        break
      fi
      if [[ "$body" == *'The operation was canceled'* ]]; then return 1; fi
      if [[ "$body" == '##[error]'* ]]; then return 1; fi
      if [[ "$body" =~ ^Sent[[:space:]]([0-9]{1,20})[[:space:]]of[[:space:]]([0-9]{1,20})[[:space:]]\([0-9]+[.][0-9]%\),[[:space:]][0-9]+[.][0-9][[:space:]]MBs/sec$ ]]; then
        sent="${BASH_REMATCH[1]}"
        total="${BASH_REMATCH[2]}"
        valid_runner_timestamp "$timestamp" || return 1
        [[ "$sent" =~ ^(0|[1-9][0-9]{0,19})$ ]] \
          && [[ "$total" =~ ^(0|[1-9][0-9]{0,19})$ ]] \
          && [[ "$sent" =~ ^[1-9][0-9]*$ ]] \
          && [[ "$total" =~ ^[1-9][0-9]*$ ]] \
          && decimal_less "$sent" "$total" && partial=true
      elif [[ "$body" == Sent\ * ]]; then
        return 1
      fi
    elif [[ "$line" == *'The operation was canceled'* ]]; then
      return 1
    fi
  done < "$path"
  [ "$partial" = true ] && grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{7}Z ##\[error\]The operation was canceled\.$' "$path"
}

fetch_save_step_log() {
  local job_id="$1" index="$2"
  private_root_open "$SAVE_LOG_ROOT" \
    && stock_restore_step_log "$SAVE_LOG_ROOT" "$job_id" "$index" \
    && private_file_valid "$SAVE_LOG_ROOT" "$SAVE_LOG_ROOT/restore-step.log" 1048576
}

fetch_restore_log() {
  local run_id="${GITHUB_RUN_ID:-}" attempt="${GITHUB_RUN_ATTEMPT:-}"
  local workflow_id="${WORKFLOW_ID:-}" snapshot document controller_mode controller_probe_id
  local controller_title controller_title_without_probe job_id index
  local transport="$RESTORE_LOG_ROOT" run_path="$RESTORE_LOG_ROOT/run.json"
  [[ "$run_id" =~ ^[1-9][0-9]{0,19}$ ]] || return 1
  [[ "$attempt" =~ ^[1-9][0-9]{0,9}$ ]] || return 1
  [[ "$workflow_id" =~ ^[1-9][0-9]{0,19}$ ]] || return 1
  [ "$GITHUB_REPOSITORY" = tailrocks/velnor-new ] || return 1
  [ "$GITHUB_EVENT_NAME" = workflow_dispatch ] || return 1
  [ "$GITHUB_REF" = refs/heads/main ] || return 1
  [ "$REF_PROTECTED" = true ] || return 1
  private_event_valid || return 1
  controller_mode="$(jq -er '.inputs.mode | select(type == "string")' "$GITHUB_EVENT_PATH" 2>/dev/null)" || return 1
  controller_probe_id="$(jq -er 'if (.inputs.probe_id | type) == "string" then .inputs.probe_id else error("controller probe shape") end' "$GITHUB_EVENT_PATH" 2>/dev/null)" || return 1
  [ "$controller_mode" = "$CONTROLLER_MODE" ] || return 1
  controller_title="MBX cancellation $controller_mode $controller_probe_id"
  controller_title_without_probe="MBX cancellation $controller_mode"
  stock_restore_private_dir "$transport" "$RUNNER_TEMP" || return 1
  snapshot="$(stock_restore_attempt_snapshot "$transport" "$run_id" "$attempt" \
    "$GITHUB_SHA" "$OBSERVER_JOB_NAME" in_progress success 2>/dev/null)" || return 1
  document="$(stock_restore_single_object "$run_path" 2097152)" || return 1
  jq -e --argjson id "$run_id" --argjson workflow "$workflow_id" \
    --argjson attempt "$attempt" --arg repo "$GITHUB_REPOSITORY" \
    --arg sha "$GITHUB_SHA" --arg actor "$GITHUB_ACTOR" \
    --arg title "$controller_title" --arg title_without_probe "$controller_title_without_probe" \
    --arg input_probe "$controller_probe_id" '
      .id == $id and .workflow_id == $workflow
      and .repository.full_name == $repo and .head_repository.full_name == $repo
      and .event == "workflow_dispatch" and .head_branch == "main" and .head_sha == $sha
      and .run_attempt == $attempt and .actor.login == $actor and .status == "in_progress"
      and (.display_title == $title
        or ($input_probe == "" and .display_title == $title_without_probe))
      and (.path == ".github/workflows/qualification.yml@main"
        or .path == ".github/workflows/qualification.yml@refs/heads/main")
    ' <<< "$document" >/dev/null 2>&1 || return 1
  read -r workflow_id job_id index <<< "$snapshot"
  [[ "$job_id" =~ ^[1-9][0-9]{0,19}$ ]] && [[ "$index" =~ ^[0-9]{1,6}$ ]] || return 1
  stock_restore_clean_log "$transport" "$job_id" "$index" "$DERIVED_KEY" miss
}
"#;
