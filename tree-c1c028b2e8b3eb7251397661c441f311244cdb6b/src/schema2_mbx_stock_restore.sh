stock_restore_private_file() {
  local path="$1" root="$2" max_bytes="$3" resolved owner links bytes uid
  case "$path" in "$root"/*) ;; *) return 1 ;; esac
  [ -f "$path" ] && [ ! -L "$path" ] || return 1
  resolved="$(realpath -e -- "$path" 2>/dev/null)" || return 1
  [ "$resolved" = "$path" ] || return 1
  owner="$(stat -c '%u' -- "$path" 2>/dev/null)" || return 1
  links="$(stat -c '%h' -- "$path" 2>/dev/null)" || return 1
  bytes="$(stat -c '%s' -- "$path" 2>/dev/null)" || return 1
  uid="$(id -u)" || return 1
  [[ "$owner" =~ ^[0-9]+$ ]] && [ "$owner" = "$uid" ] \
    && [ "$links" = 1 ] && [[ "$bytes" =~ ^[0-9]+$ ]] \
    && [ "$bytes" -gt 0 ] && [ "$bytes" -le "$max_bytes" ]
}

stock_restore_api_json() {
  local path="$1" destination="$2" status_path="${2}.status" status
  [ ! -e "$destination" ] && [ ! -L "$destination" ] \
    && [ ! -e "$status_path" ] && [ ! -L "$status_path" ] || return 1
  (
    ulimit -f 4096
    curl --disable --noproxy '*' --proto '=https' --max-redirs 0 \
      --connect-timeout 15 --max-time 30 --max-filesize 2097152 \
      --silent --show-error --fail --write-out '%{http_code}' \
      --header "Authorization: Bearer $GH_TOKEN" \
      --header 'Accept: application/vnd.github+json' \
      --header 'X-GitHub-Api-Version: 2022-11-28' \
      --output "$destination" "https://api.github.com$path" \
      > "$status_path" 2>/dev/null
  ) || return 1
  stock_restore_private_file "$destination" "$RUNNER_TEMP" 2097152 \
    && stock_restore_private_file "$status_path" "$RUNNER_TEMP" 3 || return 1
  status="$(cat "$status_path" 2>/dev/null || true)"
  [ "$status" = 200 ]
}

stock_restore_safe_https_url() {
  local url="$1" rest authority
  case "$url" in https://*) rest="${url#https://}" ;; *) return 1 ;; esac
  case "$url" in *[[:space:][:cntrl:]]*|*'"'*|*'\'*|*'#'*) return 1 ;; esac
  case "$rest" in */*) authority="${rest%%/*}" ;; *) return 1 ;; esac
  [ -n "$authority" ] && [ "${#url}" -le 8192 ] || return 1
  case "$authority" in *@*|*'?'*|*'#'*) return 1 ;; esac
}

stock_restore_fetch_signed_log() {
  local root="$1" url="$2" status header_bytes bytes
  stock_restore_safe_https_url "$url" || return 1
  (
    ulimit -f 2048
    curl --disable --proto '=https' --max-redirs 0 --connect-timeout 15 --max-time 30 \
      --max-filesize 1048576 --silent --show-error --fail \
      --dump-header "$root/log-signed.headers" --output "$root/restore-step.log" \
      --write-out '%{http_code}' -- "$url" \
      > "$root/log-signed.status" 2> "$root/log-signed.stderr"
  ) || return 1
  status="$(cat "$root/log-signed.status" 2>/dev/null || true)"
  header_bytes="$(wc -c < "$root/log-signed.headers" 2>/dev/null | tr -d ' ' || printf 65537)"
  bytes="$(wc -c < "$root/restore-step.log" 2>/dev/null | tr -d ' ' || printf 1048577)"
  [ "$status" = 200 ] && [ "$header_bytes" -le 65536 ] && [ "$bytes" -le 1048576 ]
}

stock_restore_step_log() {
  local root="$1" job_id="$2" step_index="$3" endpoint status header_bytes locations location
  endpoint="https://api.github.com/repos/$GITHUB_REPOSITORY/actions/jobs/$job_id/steps/$step_index/logs"
  (
    ulimit -f 128
    curl --disable --noproxy '*' --proto '=https' --max-redirs 0 \
      --connect-timeout 15 --max-time 30 --max-filesize 65536 \
      --silent --show-error --dump-header "$root/log-api.headers" --output /dev/null \
      --write-out '%{http_code}' --header "Authorization: Bearer $GH_TOKEN" \
      --header 'Accept: application/vnd.github+json' \
      --header 'X-GitHub-Api-Version: 2022-11-28' \
      -- "$endpoint" > "$root/log-api.status" 2> "$root/log-api.stderr"
  ) || return 1
  status="$(cat "$root/log-api.status" 2>/dev/null || true)"
  header_bytes="$(wc -c < "$root/log-api.headers" 2>/dev/null | tr -d ' ' || printf 65537)"
  locations="$(awk 'tolower($1) == "location:" { count++ } END { print count+0 }' "$root/log-api.headers" 2>/dev/null || printf 0)"
  [ "$status" = 302 ] && [ "$header_bytes" -le 65536 ] && [ "$locations" = 1 ] || return 1
  location="$(awk 'tolower($1) == "location:" { sub(/^[^:]*:[[:space:]]*/, ""); sub(/\r$/, ""); print }' "$root/log-api.headers")"
  stock_restore_fetch_signed_log "$root" "$location"
}

stock_restore_clean_log() {
  local root="$1" job_id="$2" step_index="$3" key="$4" expected="$5"
  local line timestamp body normalized hit_count=0 miss_count=0 malformed=0
  stock_restore_step_log "$root" "$job_id" "$step_index" || return 1
  grep -Eiq 'FailedToRestore|Failed to restore|Event Validation Error:|(^|[^[:alpha:]])warnings?([^[:alpha:]]|$)|(^|[^[:alpha:]])errors?([^[:alpha:]]|$)|##\[(warning|error)\]' \
    "$root/restore-step.log" && return 1
  while IFS= read -r line || [ -n "$line" ]; do
    if [[ "$line" =~ ^([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{7}Z)\ (.*)$ ]]; then
      timestamp="${BASH_REMATCH[1]}"
      body="${BASH_REMATCH[2]}"
      normalized="$(date -u -d "$timestamp" '+%Y-%m-%dT%H:%M:%S.%7NZ' 2>/dev/null || printf invalid)"
      if [ "$normalized" = "$timestamp" ]; then
        case "$body" in
          "Cache restored from key: $key") hit_count=$((hit_count + 1)) ;;
          "Cache not found for input keys: $key") miss_count=$((miss_count + 1)) ;;
          *"Cache restored from key:"*|*"Cache not found for input keys:"*) malformed=1 ;;
          *) ;;
        esac
      elif [[ "$body" == *"Cache restored from key:"* || "$body" == *"Cache not found for input keys:"* ]]; then
        malformed=1
      fi
    elif [[ "$line" == *"Cache restored from key:"* || "$line" == *"Cache not found for input keys:"* ]]; then
      malformed=1
    fi
  done < "$root/restore-step.log"
  [ "$malformed" = 0 ] || return 1
  case "$expected" in
    hit) [ "$hit_count" = 1 ] && [ "$miss_count" = 0 ] ;;
    miss) [ "$miss_count" = 1 ] && [ "$hit_count" = 0 ] ;;
    *) return 1 ;;
  esac
}

stock_restore_validate_receipt() {
  local receipt="$1" job="$2" role="$3" run="$4" attempt="$5" sha="$6" ref="$7" workflow="$8"
  stock_restore_private_file "$receipt" "$RUNNER_TEMP" 65536 || return 1
  jq -e --arg job "$job" --arg role "$role" --arg run "$run" --arg attempt "$attempt" \
    --arg sha "$sha" --arg ref "$ref" --arg workflow "$workflow" '
      type == "object" and .receipt_status == "provisional"
      and .job_id == $job and .role == $role
      and .run_id == $run and .run_attempt == $attempt
      and .source_sha == $sha and .source_ref == $ref and .workflow_ref == $workflow
      and (.primary_key | type == "string" and test("^[a-z0-9][a-z0-9.-]{0,511}$"))
      and .derived_primary_key == .primary_key
      and (.restore_primary_key | type == "string"
        and (. == "" or test("^[a-z0-9][a-z0-9.-]{0,511}$")))
      and (.restore_conclusion | type == "string" and length <= 32)
      and (.cache_hit | type == "string" and IN("", "true", "false"))
      and (.matched_key | type == "string"
        and (. == "" or test("^[a-z0-9][a-z0-9.-]{0,511}$")))
    ' "$receipt" >/dev/null 2>&1
}

stock_restore_write_evidence() {
  local target="$1" logical_job="$2" role="$3" job_name="$4"
  local run="$5" attempt="$6" sha="$7" ref="$8" workflow="$9"
  local workflow_id="${10}" api_job_id="${11}" step_index="${12}" derived="${13}"
  local actual="${14}" hit="${15}" matched="${16}" conclusion="${17}"
  local classification="${18}"
  local root="$RUNNER_TEMP/mbx-stock-restore-evidence" name uid
  [ -d "$root" ] && [ ! -L "$root" ] || return 1
  [ "$(realpath -e -- "$root" 2>/dev/null)" = "$root" ] || return 1
  [ "$(stat -c '%a' -- "$root" 2>/dev/null)" = 700 ] || return 1
  uid="$(id -u)" || return 1
  [ "$(stat -c '%u' -- "$root" 2>/dev/null)" = "$uid" ] || return 1
  case "$target" in "$root"/*) name="${target#"$root"/}" ;; *) return 1 ;; esac
  [[ "$name" =~ ^[a-z0-9][a-z0-9._-]{0,63}\.json$ ]] || return 1
  [ ! -e "$target" ] && [ ! -L "$target" ] || return 1
  temp_file="$(mktemp "$root/.stock-restore.XXXXXXXXXX" 2>/dev/null)" || return 1
  chmod 600 "$temp_file" || { rm -f -- "$temp_file"; return 1; }
  jq -n --arg classification "$classification" --arg logical_job_id "$logical_job" \
    --arg role "$role" --arg job_name "$job_name" --arg run_id "$run" \
    --arg run_attempt "$attempt" --arg source_sha "$sha" --arg source_ref "$ref" \
    --arg workflow_ref "$workflow" --arg workflow_id "$workflow_id" \
    --arg api_job_id "$api_job_id" --arg restore_step 'Restore MBX single bundle' \
    --arg restore_step_index "$step_index" --arg derived_primary_key "$derived" \
    --arg restore_primary_key "$actual" --arg cache_hit "$hit" \
    --arg matched_key "$matched" --arg restore_conclusion "$conclusion" \
    '{schema_version:1,classification:$classification,logical_job_id:$logical_job_id,
      role:$role,job_name:$job_name,run_id:$run_id,run_attempt:$run_attempt,
      source_sha:$source_sha,source_ref:$source_ref,workflow_ref:$workflow_ref,
      workflow_id:($workflow_id|tonumber),api_job_id:($api_job_id|tonumber),restore_step:$restore_step,
      restore_step_index:($restore_step_index|tonumber),derived_primary_key:$derived_primary_key,
      restore_primary_key:$restore_primary_key,cache_hit:$cache_hit,
      matched_key:$matched_key,restore_conclusion:$restore_conclusion}' \
    >| "$temp_file" 2>/dev/null || { rm -f -- "$temp_file"; return 1; }
  ln -- "$temp_file" "$target" 2>/dev/null || { rm -f -- "$temp_file"; return 1; }
  rm -f -- "$temp_file"
}

stock_restore_context_valid() {
  local mode="$1" repository=tailrocks/velnor-new
  [ "$mode" = mbx-cache-roundtrip ] || [ "$mode" = mbx-cache-parallel ] || return 1
  [ "${GITHUB_REPOSITORY:-}" = "$repository" ] \
    && [ "${GITHUB_REF:-}" = refs/heads/main ] \
    && [ "${GITHUB_REF_PROTECTED:-}" = true ] \
    && [ "${GITHUB_EVENT_NAME:-}" = workflow_dispatch ] \
    && [ "${GITHUB_WORKFLOW_REF:-}" = "$repository/.github/workflows/qualification.yml@refs/heads/main" ] \
    && [ -n "${GH_TOKEN:-}" ] && [ -n "${RUNNER_TEMP:-}" ] \
    && [ -d "$RUNNER_TEMP" ] && [ ! -L "$RUNNER_TEMP" ] \
    && [ "$(realpath -e -- "$RUNNER_TEMP" 2>/dev/null)" = "$RUNNER_TEMP" ] \
    && [ -n "${GITHUB_EVENT_PATH:-}" ] \
    && [[ "${GITHUB_RUN_ID:-}" =~ ^[1-9][0-9]{0,19}$ ]] \
    && [[ "${GITHUB_RUN_ATTEMPT:-}" =~ ^[1-9][0-9]{0,9}$ ]] \
    && [[ "${GITHUB_SHA:-}" =~ ^[0-9a-f]{40}$ ]] \
    && stock_restore_private_file "$GITHUB_EVENT_PATH" "$RUNNER_TEMP" 65536 \
    && jq -e --arg mode "$mode" '(.inputs | type == "object") and .inputs.mode == $mode' \
      "$GITHUB_EVENT_PATH" >/dev/null 2>&1
}

stock_restore_api_snapshot() {
  local root="$1" run="$2" attempt="$3" sha="$4"
  local repository=tailrocks/velnor-new run_json="$1/run.json" jobs_json="$1/jobs.json"
  stock_restore_api_json "/repos/$repository/actions/runs/$run" "$run_json" \
    && stock_restore_api_json "/repos/$repository/actions/runs/$run/attempts/$attempt/jobs?per_page=100" "$jobs_json" \
    && jq -e --argjson id "$run" --argjson attempt "$attempt" \
      --arg repo "$repository" --arg sha "$sha" '
        type == "object" and .id == $id and .run_attempt == $attempt and .head_sha == $sha
        and .repository.full_name == $repo and .head_repository.full_name == $repo
        and .event == "workflow_dispatch" and .head_branch == "main"
        and ((.path | split("@") | .[0]) == ".github/workflows/qualification.yml")
        and (.path | endswith("@refs/heads/main"))
        and (.workflow_id | type == "number" and . > 0 and . == floor)
      ' "$run_json" >/dev/null 2>&1 \
    && jq -e --arg run "$run" --arg attempt "$attempt" --arg sha "$sha" '
      type == "object" and (.jobs | type == "array")
      and (.total_count | type == "number") and .total_count >= 0
      and .total_count == (.total_count | floor) and .total_count == (.jobs | length)
      and all(.jobs[]; type == "object"
        and (.id | type == "number" and . > 0 and . == floor)
        and (.run_id | type == "number") and (.run_id | tostring) == $run
        and (if has("run_attempt") then
          (.run_attempt | type == "number") and (.run_attempt | tostring) == $attempt
          else true end)
        and (.head_sha | type == "string" and . == $sha)
        and (.name | type == "string" and length > 0)
        and (.status | IN("queued", "in_progress", "completed"))
        and (if .status == "completed" then (.conclusion | type == "string")
          else (.conclusion == null or (.conclusion | type == "string")) end)
        and (.steps | type == "array")
        and all(.steps[]; type == "object" and (.name | type == "string")
          and (.status | IN("queued", "in_progress", "completed"))
          and (if .status == "completed" then (.conclusion | type == "string")
            else (.conclusion == null or (.conclusion | type == "string")) end)))' \
      "$jobs_json" >/dev/null 2>&1
}

stock_restore_step_identity() {
  local jobs_json="$1" name="$2" status="$3" run="$4" attempt="$5" sha="$6" conclusion="$7"
  jq -er --arg name "$name" --arg status "$status" --arg run "$run" \
    --arg attempt "$attempt" --arg sha "$sha" --arg conclusion "$conclusion" \
    --arg step 'Restore MBX single bundle' '
      [.jobs[] | select(.name == $name and .status == $status
        and (.run_id | type == "number") and (.run_id | tostring) == $run
        and (if has("run_attempt") then
          (.run_attempt | type == "number") and (.run_attempt | tostring) == $attempt
          else true end)
        and .head_sha == $sha
        and ($status != "completed" or .conclusion == "success"))] as $jobs
      | if ($jobs | length) != 1 or ($jobs[0].steps | type) != "array" then error("job steps")
        else $jobs[0] as $job
          | ($job.steps | to_entries | map(select(.value.name == $step))) as $steps
          | if ($steps | length) != 1 or $steps[0].value.status != "completed"
              or $steps[0].value.conclusion != "success" or $conclusion != "success"
            then error("restore step")
            elif ($job.id | type) != "number" or $job.id <= 0 or $job.id != ($job.id | floor)
              then error("job id")
            else "\($job.id)\t\($steps[0].key)"
          end
        end' "$jobs_json" 2>/dev/null
}

stock_restore_attempt_snapshot() {
  local root="$1" run="$2" attempt="$3" sha="$4" name="$5" status="$6" conclusion="$7"
  local workflow_id identity
  stock_restore_api_snapshot "$root" "$run" "$attempt" "$sha" || return 1
  workflow_id="$(jq -er '.workflow_id | select(type == "number" and . > 0 and . == floor)' \
    "$root/run.json" 2>/dev/null)" || return 1
  identity="$(stock_restore_step_identity "$root/jobs.json" "$name" "$status" \
    "$run" "$attempt" "$sha" "$conclusion")" || return 1
  printf '%s\t%s\n' "$workflow_id" "$identity"
}

stock_restore_classify_receipt() {
  if [ "$#" -lt 6 ] || [ "$#" -gt 7 ]; then printf '%s\n' NOT_RUN; return 0; fi
  local receipt="$1" expected_job_id="$2" expected_role="$3" expected_job_name="$4"
  local expected_job_status="$5" expected_mode="$6" evidence_path="${7:-}"
  local temp="" snapshot="" run_id="${GITHUB_RUN_ID:-}" attempt="${GITHUB_RUN_ATTEMPT:-}"
  local sha="${GITHUB_SHA:-}" ref="${GITHUB_REF:-}" workflow="${GITHUB_WORKFLOW_REF:-}"
  local workflow_id="" job_id="" step_index="" derived="" actual_primary=""
  local hit="" matched="" conclusion="" outcome=NOT_RUN uid
  if ! stock_restore_context_valid "$expected_mode" \
    || { [ "$expected_job_status" != completed ] && [ "$expected_job_status" != in_progress ]; } \
    || ! stock_restore_validate_receipt "$receipt" "$expected_job_id" "$expected_role" \
      "$run_id" "$attempt" "$sha" "$ref" "$workflow"; then
    printf '%s\n' NOT_RUN; return 0
  fi
  uid="$(id -u)" || { printf '%s\n' NOT_RUN; return 0; }
  derived="$(jq -r '.primary_key' "$receipt")" || { printf '%s\n' NOT_RUN; return 0; }
  actual_primary="$(jq -r '.restore_primary_key' "$receipt")" || { printf '%s\n' NOT_RUN; return 0; }
  hit="$(jq -r '.cache_hit' "$receipt")" || { printf '%s\n' NOT_RUN; return 0; }
  matched="$(jq -r '.matched_key' "$receipt")" || { printf '%s\n' NOT_RUN; return 0; }
  conclusion="$(jq -r '.restore_conclusion' "$receipt")" || { printf '%s\n' NOT_RUN; return 0; }
  temp="$(mktemp -d "$RUNNER_TEMP/mbx-stock-restore.XXXXXXXXXX" 2>/dev/null || true)"
  if [ -n "$temp" ] && [ -d "$temp" ] && [ ! -L "$temp" ] \
    && [ "$(realpath -e -- "$temp" 2>/dev/null)" = "$temp" ] \
    && [ "$(stat -c '%a:%u' -- "$temp" 2>/dev/null)" = "700:$uid" ]; then
    snapshot="$(stock_restore_attempt_snapshot "$temp" "$run_id" "$attempt" "$sha" \
      "$expected_job_name" "$expected_job_status" "$conclusion" 2>/dev/null || true)"
    if [ -n "$snapshot" ]; then
      read -r workflow_id job_id step_index <<< "$snapshot"
      if [[ "$job_id" =~ ^[1-9][0-9]{0,19}$ ]] && [[ "$step_index" =~ ^[0-9]{1,6}$ ]] \
        && [ "$actual_primary" = "$derived" ]; then
        if [ "$hit" = true ] && [ "$matched" = "$derived" ] \
          && stock_restore_clean_log "$temp" "$job_id" "$step_index" "$derived" hit; then
          outcome=HIT
        elif [ -z "$hit" ] && [ -z "$matched" ] \
          && stock_restore_clean_log "$temp" "$job_id" "$step_index" "$derived" miss; then
          outcome=CLEAN_MISS
        fi
      fi
    fi
  fi
  if [ -n "$temp" ]; then
    if [ -d "$temp" ] && [ ! -L "$temp" ]; then
      rm -rf -- "$temp" || outcome=NOT_RUN
    else
      outcome=NOT_RUN
    fi
  fi
  if [ "$#" -eq 7 ] && [ "$outcome" != NOT_RUN ]; then
    if ! stock_restore_write_evidence "$evidence_path" "$expected_job_id" \
      "$expected_role" "$expected_job_name" "$run_id" "$attempt" "$sha" "$ref" \
      "$workflow" "$workflow_id" "$job_id" "$step_index" "$derived" \
      "$actual_primary" "$hit" "$matched" "$conclusion" "$outcome"; then
      outcome=NOT_RUN
    fi
  fi
  printf '%s\n' "$outcome"
}
