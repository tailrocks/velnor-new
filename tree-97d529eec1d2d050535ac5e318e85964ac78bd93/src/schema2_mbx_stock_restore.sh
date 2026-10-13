stock_restore_api_json() {
  local path="$1" destination="$2" bytes
  (
    ulimit -f 4096
    curl --disable --proto '=https' --max-redirs 0 --connect-timeout 15 --max-time 30 \
      --max-filesize 2097152 --silent --show-error --fail \
      --header "Authorization: Bearer $GH_TOKEN" \
      --header 'Accept: application/vnd.github+json' \
      --header 'X-GitHub-Api-Version: 2022-11-28' \
      --output "$destination" "https://api.github.com$path" 2>/dev/null
  ) || return 1
  bytes="$(wc -c < "$destination" | tr -d ' ')" || return 1
  [[ "$bytes" =~ ^[0-9]+$ ]] && [ "$bytes" -le 2097152 ]
}

stock_restore_step_log() {
  local root="$1" job_id="$2" step_index="$3" endpoint status header_bytes locations location
  local signed_status signed_header_bytes log_bytes
  local -a signed_pipe
  endpoint="https://api.github.com/repos/$GITHUB_REPOSITORY/actions/jobs/$job_id/steps/$step_index/logs"
  (
    ulimit -f 128
    curl --disable --proto '=https' --max-redirs 0 --connect-timeout 15 --max-time 30 \
      --max-filesize 65536 --silent --show-error \
      --dump-header "$root/log-api.headers" --output /dev/null --write-out '%{http_code}' \
      --header "Authorization: Bearer $GH_TOKEN" \
      --header 'Accept: application/vnd.github+json' \
      --header 'X-GitHub-Api-Version: 2022-11-28' "$endpoint" \
      > "$root/log-api.status" 2> "$root/log-api.stderr"
  ) || return 1
  status="$(cat "$root/log-api.status" 2>/dev/null || true)"
  header_bytes="$(wc -c < "$root/log-api.headers" 2>/dev/null | tr -d ' ' || printf 65537)"
  locations="$(awk 'tolower($1) == "location:" { count++ } END { print count+0 }' "$root/log-api.headers" 2>/dev/null || printf 0)"
  [ "$status" = 302 ] && [ "$header_bytes" -le 65536 ] && [ "$locations" = 1 ] || return 1
  location="$(awk 'tolower($1) == "location:" { sub(/^[^:]*:[[:space:]]*/, ""); sub(/\r$/, ""); print }' "$root/log-api.headers")"
  case "$location" in https://*) ;; *) return 1 ;; esac
  case "$location" in *[[:space:][:cntrl:]]*|*'"'*|*'\'*) return 1 ;; esac
  printf 'url = "%s"\n' "$location" > "$root/log-signed.curlrc"
  set +o pipefail
  (
    ulimit -f 2048
    curl --disable --config "$root/log-signed.curlrc" --proto '=https' --proto-redir '=https' \
      --max-redirs 2 --connect-timeout 15 --max-time 30 --max-filesize 1048576 \
      --silent --fail --location --dump-header "$root/log-signed.headers" --output - \
      2> "$root/log-signed.stderr"
  ) | head -c 1048577 > "$root/restore-step.log"
  signed_pipe=("${PIPESTATUS[@]}")
  set -o pipefail
  log_bytes="$(wc -c < "$root/restore-step.log" | tr -d ' ' || printf 1048577)"
  signed_header_bytes="$(wc -c < "$root/log-signed.headers" 2>/dev/null | tr -d ' ' || printf 65537)"
  signed_status="$(awk '$1 ~ /^HTTP\// { status=$2 } END { print status }' "$root/log-signed.headers" 2>/dev/null || true)"
  [ "${signed_pipe[0]}" = 0 ] && [ "${signed_pipe[1]}" = 0 ] \
    && [ "$log_bytes" -le 1048576 ] && [ "$signed_header_bytes" -le 65536 ] \
    && [ "$signed_status" = 200 ]
}

stock_restore_clean_miss_log() {
  local root="$1" job_id="$2" step_index="$3" key="$4" line timestamp body normalized
  local clean_count=0
  stock_restore_step_log "$root" "$job_id" "$step_index" || return 1
  grep -Eiq 'FailedToRestore|Failed to restore|Event Validation Error:|warning|error|##\[warning\]|##\[error\]' \
    "$root/restore-step.log" && return 1
  while IFS= read -r line || [ -n "$line" ]; do
    if [[ "$line" =~ ^([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{7}Z)\ (.*)$ ]]; then
      timestamp="${BASH_REMATCH[1]}"
      body="${BASH_REMATCH[2]}"
      normalized="$(date -u -d "$timestamp" '+%Y-%m-%dT%H:%M:%S.%7NZ' 2>/dev/null || printf invalid)"
      if [ "$normalized" = "$timestamp" ] && [ "$body" = "Cache not found for input keys: $key" ]; then
        clean_count=$((clean_count + 1))
      fi
    fi
  done < "$root/restore-step.log"
  [ "$clean_count" = 1 ]
}

stock_restore_classify_receipt() {
  if [ "$#" -lt 6 ] || [ "$#" -gt 7 ]; then
    printf '%s\n' NOT_RUN
    return 0
  fi
  local receipt="$1" expected_job_id="$2" expected_role="$3" expected_job_name="$4"
  local expected_job_status="$5" expected_mode="$6" evidence_path="${7:-}"
  local temp run_json jobs_json run_id="" attempt="" sha="" ref="" workflow="" workflow_id=""
  local derived="" actual_primary="" hit="" matched="" conclusion="" job_id="" step_index="" outcome=NOT_RUN
  local github_repository=tailrocks/velnor-new
  if [ ! -f "$receipt" ] || [ -L "$receipt" ]; then
    printf '%s\n' NOT_RUN
    return 0
  fi
  if [ "${GITHUB_REPOSITORY:-}" != "$github_repository" ] \
    || [ "${GITHUB_REF:-}" != refs/heads/main ] || [ "${GITHUB_REF_PROTECTED:-}" != true ] \
    || [ "${GITHUB_EVENT_NAME:-}" != workflow_dispatch ] \
    || [ "${GITHUB_WORKFLOW_REF:-}" != "$github_repository/.github/workflows/qualification.yml@refs/heads/main" ] \
    || [ -z "${GH_TOKEN:-}" ] || [ -z "${RUNNER_TEMP:-}" ] || [ ! -d "$RUNNER_TEMP" ] \
    || [ -L "$RUNNER_TEMP" ] || [ ! -f "${GITHUB_EVENT_PATH:-}" ]; then
    printf '%s\n' NOT_RUN
    return 0
  fi
  if ! jq -e --arg mode "$expected_mode" '.inputs.mode == $mode' "$GITHUB_EVENT_PATH" >/dev/null 2>&1; then
    printf '%s\n' NOT_RUN
    return 0
  fi
  if ! jq -e --arg job "$expected_job_id" --arg role "$expected_role" \
    --arg run "${GITHUB_RUN_ID:-}" --arg attempt "${GITHUB_RUN_ATTEMPT:-}" \
    --arg sha "${GITHUB_SHA:-}" --arg ref "$GITHUB_REF" --arg workflow "$GITHUB_WORKFLOW_REF" '
      type == "object" and .receipt_status == "provisional"
      and .job_id == $job and .role == $role
      and .run_id == $run and .run_attempt == $attempt
      and .source_sha == $sha and .source_ref == $ref and .workflow_ref == $workflow
      and (.primary_key | type == "string" and test("^[a-z0-9-]+$"))
      and .derived_primary_key == .primary_key
      and (.restore_primary_key | type == "string" and (. == "" or test("^[a-z0-9-]+$")))
      and (.restore_conclusion | type == "string")
      and (.cache_hit | type == "string") and (.matched_key | type == "string")
    ' "$receipt" >/dev/null 2>&1; then
    printf '%s\n' NOT_RUN
    return 0
  fi
  run_id="${GITHUB_RUN_ID:-}"
  attempt="${GITHUB_RUN_ATTEMPT:-}"
  sha="${GITHUB_SHA:-}"
  ref="$GITHUB_REF"
  workflow="$GITHUB_WORKFLOW_REF"
  if [[ ! "$run_id" =~ ^[1-9][0-9]{0,19}$ ]] || [[ ! "$attempt" =~ ^[1-9][0-9]{0,9}$ ]] \
    || [[ ! "$sha" =~ ^[0-9a-f]{40}$ ]] \
    || { [ "$expected_job_status" != completed ] && [ "$expected_job_status" != in_progress ]; }; then
    printf '%s\n' NOT_RUN
    return 0
  fi
  derived="$(jq -r '.primary_key' "$receipt")"
  actual_primary="$(jq -r '.restore_primary_key' "$receipt")"
  hit="$(jq -r '.cache_hit' "$receipt")"
  matched="$(jq -r '.matched_key' "$receipt")"
  conclusion="$(jq -r '.restore_conclusion' "$receipt")"
  temp="$(mktemp -d "$RUNNER_TEMP/mbx-stock-restore.XXXXXXXXXX" 2>/dev/null || true)"
  if [ -n "$temp" ] && [ -d "$temp" ] && [ ! -L "$temp" ] \
    && chmod 700 "$temp"; then
    run_json="$temp/run.json"
    jobs_json="$temp/jobs.json"
    if stock_restore_api_json "/repos/$github_repository/actions/runs/$run_id" "$run_json" \
      && stock_restore_api_json "/repos/$github_repository/actions/runs/$run_id/attempts/$attempt/jobs?per_page=100" "$jobs_json" \
      && jq -e --argjson id "$run_id" --argjson attempt "$attempt" --arg repo "$github_repository" \
        --arg sha "$sha" --arg workflow "$workflow" '
          .id == $id and .run_attempt == $attempt and .head_sha == $sha
          and .repository.full_name == $repo and .head_repository.full_name == $repo
          and .event == "workflow_dispatch" and .head_branch == "main"
          and ((.path | split("@") | .[0]) == ".github/workflows/qualification.yml")
          and (.path | endswith("@refs/heads/main"))
          and (.workflow_id | type == "number" and . > 0 and . == floor)
        ' "$run_json" >/dev/null 2>&1 \
      && jq -e '.total_count == (.jobs | length) and (.jobs | type == "array")' "$jobs_json" >/dev/null 2>&1; then
      workflow_id="$(jq -er '.workflow_id | select(type == "number" and . > 0 and . == floor)' "$run_json" 2>/dev/null || true)"
      job_id="$(jq -er --arg name "$expected_job_name" --arg status "$expected_job_status" \
        '[.jobs[] | select(.name == $name and .status == $status
          and ($status != "completed" or .conclusion == "success"))]
         | if length == 1 then .[0].id | select(type == "number" and . > 0 and . == floor)
           else error("job identity") end' "$jobs_json" 2>/dev/null || true)"
      step_index="$(jq -er --arg name "$expected_job_name" --arg status "$expected_job_status" \
        --arg step 'Restore MBX single bundle' --arg conclusion "$conclusion" \
        '[.jobs[] | select(.name == $name and .status == $status
          and ($status != "completed" or .conclusion == "success"))] as $jobs
         | if ($jobs | length) != 1 or ($jobs[0].steps | type) != "array" then error("job steps")
           else ($jobs[0].steps | to_entries | map(select(.value.name == $step))) as $steps
             | if ($steps | length) != 1 or $steps[0].value.status != "completed"
                 or $steps[0].value.conclusion != "success" or $conclusion != "success"
               then error("restore step")
               else $steps[0].key | tonumber
                 | select(type == "number" and . >= 0 and . < ($jobs[0].steps | length) and . == floor)
             end
           end' "$jobs_json" 2>/dev/null || true)"
      if [[ "$job_id" =~ ^[1-9][0-9]{0,19}$ ]] && [[ "$step_index" =~ ^[0-9]{1,6}$ ]] \
        && [ "$actual_primary" = "$derived" ]; then
        if [ "$hit" = true ] && [ "$matched" = "$derived" ]; then
          outcome=HIT
        elif [ -z "$hit" ] && [ -z "$matched" ] \
          && stock_restore_clean_miss_log "$temp" "$job_id" "$step_index" "$derived"; then
          outcome=CLEAN_MISS
        fi
      fi
    fi
  fi
  if [ -n "${temp:-}" ] && [ -d "$temp" ] && [ ! -L "$temp" ]; then
    rm -rf -- "$temp"
  fi
  if [ "$#" -eq 7 ] && [ "$outcome" != NOT_RUN ]; then
    local evidence_path="$7"
    case "$evidence_path" in "$RUNNER_TEMP"/mbx-stock-restore-evidence/*) ;; *) outcome=NOT_RUN ;; esac
    if [ "$outcome" != NOT_RUN ] && [ ! -e "$evidence_path" ] && [ ! -L "$evidence_path" ]; then
      umask 077
      jq -n --arg classification "$outcome" --arg logical_job_id "$expected_job_id" \
        --arg role "$expected_role" --arg job_name "$expected_job_name" \
        --arg run_id "$run_id" --arg run_attempt "$attempt" --arg source_sha "$sha" \
        --arg source_ref "$ref" --arg workflow_ref "$workflow" --arg workflow_id "$workflow_id" \
        --arg api_job_id "$job_id" --arg restore_step 'Restore MBX single bundle' \
        --arg restore_step_index "$step_index" --arg derived_primary_key "$derived" \
        --arg restore_primary_key "$actual_primary" --arg cache_hit "$hit" \
        --arg matched_key "$matched" --arg restore_conclusion "$conclusion" \
        '{schema_version:1,classification:$classification,logical_job_id:$logical_job_id,
          role:$role,job_name:$job_name,run_id:$run_id,run_attempt:$run_attempt,
          source_sha:$source_sha,source_ref:$source_ref,workflow_ref:$workflow_ref,
          workflow_id:$workflow_id,api_job_id:$api_job_id,restore_step:$restore_step,
          restore_step_index:$restore_step_index,derived_primary_key:$derived_primary_key,
          restore_primary_key:$restore_primary_key,cache_hit:$cache_hit,
          matched_key:$matched_key,restore_conclusion:$restore_conclusion}' \
        > "$evidence_path" 2>/dev/null || outcome=NOT_RUN
    else
      outcome=NOT_RUN
    fi
  fi
  printf '%s\n' "$outcome"
}
