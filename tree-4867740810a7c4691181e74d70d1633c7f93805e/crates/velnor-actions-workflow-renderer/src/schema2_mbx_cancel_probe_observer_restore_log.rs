//! Bounded retrieval of the observer's exact restore-step log.

pub(super) const RESTORE_LOG_HELPER: &str = r#"
fetch_restore_log() {
  local run_id="${GITHUB_RUN_ID:-}" attempt="${GITHUB_RUN_ATTEMPT:-}"
  local workflow_id="${WORKFLOW_ID:-}"
  local run_path="$root/restore-run.json" jobs_path="$root/restore-jobs.json"
  local job_id index api_url status header_size locations location signed_status log_size
  local timestamp body normalized
  local -a api_pipe signed_pipe
  [[ "$run_id" =~ ^[1-9][0-9]{0,19}$ ]] || return 1
  [[ "$attempt" =~ ^[1-9][0-9]{0,9}$ ]] || return 1
  [[ "$workflow_id" =~ ^[1-9][0-9]{0,19}$ ]] || return 1
  [ "$GITHUB_REPOSITORY" = tailrocks/velnor-new ] || return 1
  [ "$GITHUB_EVENT_NAME" = workflow_dispatch ] || return 1
  [ "$GITHUB_REF" = refs/heads/main ] || return 1
  [ "$REF_PROTECTED" = true ] || return 1
  gh_api --method GET "/repos/$GITHUB_REPOSITORY/actions/runs/$run_id" \
    > "$run_path" 2>/dev/null || return 1
  jq -e --argjson id "$run_id" --argjson workflow "$workflow_id" --argjson attempt "$attempt" \
    --arg repo "$GITHUB_REPOSITORY" --arg sha "$GITHUB_SHA" \
    --arg actor "$GITHUB_ACTOR" --arg title "MBX cancellation $VICTIM_MODE $PROBE_ID" \
    '.id == $id and .workflow_id == $workflow
     and .repository.full_name == $repo and .head_repository.full_name == $repo
     and .event == "workflow_dispatch" and .head_branch == "main" and .head_sha == $sha
     and .run_attempt == $attempt and .actor.login == $actor and .display_title == $title
     and ((.path | split("@") | .[0]) == ".github/workflows/qualification.yml")
     and ((.path | endswith("@refs/heads/main")))' \
    "$run_path" >/dev/null 2>&1 || return 1
  gh_api --method GET "/repos/$GITHUB_REPOSITORY/actions/runs/$run_id/attempts/$attempt/jobs?per_page=100" \
    > "$jobs_path" 2>/dev/null || return 1
  job_id="$(jq -er --arg name "$OBSERVER_JOB_NAME" \
    '[.jobs[] | select(.name == $name and .status == "in_progress")] | if length == 1 then .[0].id | select(type == "number" and . > 0 and . == floor) else error("observer job identity") end' \
    "$jobs_path" 2>/dev/null)" || return 1
  index="$(jq -er --arg name "$OBSERVER_JOB_NAME" \
    '[.jobs[] | select(.name == $name and .status == "in_progress")] as $jobs |
     if ($jobs | length) != 1 or ($jobs[0].steps | type) != "array" then error("observer steps shape")
     else ($jobs[0].steps | to_entries | map(select(.value.name == "Restore MBX single bundle"))) as $steps |
       if ($steps | length) != 1 or $steps[0].value.status != "completed"
         or $steps[0].value.conclusion != "success" then error("restore step not successful")
       else $steps[0].key | tonumber | select(type == "number" and . >= 0
         and . < ($jobs[0].steps | length) and . == floor) end end' \
    "$jobs_path" 2>/dev/null)" || return 1
  [[ "$job_id" =~ ^[1-9][0-9]{0,19}$ ]] || return 1
  [[ "$index" =~ ^[0-9]{1,6}$ ]] || return 1
  api_url="https://api.github.com/repos/tailrocks/velnor-new/actions/jobs/$job_id/steps/$index/logs"
  (
    ulimit -f 128
    curl --disable --proto '=https' --max-redirs 0 --connect-timeout 15 --max-time 30 \
      --max-filesize 65536 --silent --show-error --fail \
      --dump-header "$root/restore-log-api.headers" --output /dev/null --write-out '%{http_code}' \
      --header "Authorization: Bearer $GH_TOKEN" \
      --header 'Accept: application/vnd.github+json' \
      --header 'X-GitHub-Api-Version: 2022-11-28' "$api_url" \
      > "$root/restore-log-api.status" 2> "$root/restore-log-api.stderr"
  ) || return 1
  status="$(cat "$root/restore-log-api.status" 2>/dev/null || true)"
  header_size="$(wc -c < "$root/restore-log-api.headers" 2>/dev/null | tr -d ' ' || printf 65537)"
  locations="$(awk 'tolower($1) == "location:" { count++ } END { print count+0 }' "$root/restore-log-api.headers" 2>/dev/null || printf 0)"
  [ "$status" = 302 ] && [ "$header_size" -le 65536 ] && [ "$locations" = 1 ] || return 1
  location="$(awk 'tolower($1) == "location:" { sub(/^[^:]*:[[:space:]]*/, ""); sub(/\r$/, ""); print }' "$root/restore-log-api.headers")"
  case "$location" in https://*) ;; *) return 1 ;; esac
  case "$location" in *[[:space:][:cntrl:]]*|*'"'*|*'\'*) return 1 ;; esac
  printf 'url = "%s"\n' "$location" > "$root/restore-log.curlrc"
  set +o pipefail
  (
    ulimit -f 128
    curl --disable --config "$root/restore-log.curlrc" --proto '=https' --proto-redir '=https' \
      --max-redirs 2 --connect-timeout 15 --max-time 30 --max-filesize 1048576 \
      --silent --fail --location --dump-header "$root/restore-log-signed.headers" --output - \
      2> "$root/restore-log-signed.stderr"
  ) | head -c 1048577 > "$root/restore-step.log"
  signed_pipe=("${PIPESTATUS[@]}")
  set -o pipefail
  log_size="$(wc -c < "$root/restore-step.log" | tr -d ' ' || printf 1048577)"
  header_size="$(wc -c < "$root/restore-log-signed.headers" 2>/dev/null | tr -d ' ' || printf 65537)"
  signed_status="$(awk '$1 ~ /^HTTP\// { status=$2 } END { print status }' "$root/restore-log-signed.headers" 2>/dev/null || true)"
  [ "${signed_pipe[0]}" = 0 ] && [ "${signed_pipe[1]}" = 0 ] \
    && [ "$log_size" -le 1048576 ] && [ "$header_size" -le 65536 ] \
    && [ "$signed_status" = 200 ] || return 1
  grep -Fq 'Failed to restore:' "$root/restore-step.log" && return 1
  grep -Fq 'Event Validation Error:' "$root/restore-step.log" && return 1
  grep -Fq '##[warning]' "$root/restore-step.log" && return 1
  grep -Fq '##[error]' "$root/restore-step.log" && return 1
  local clean_count=0
  while IFS= read -r line || [ -n "$line" ]; do
    if [[ "$line" =~ ^([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{7}Z)\ (.*)$ ]]; then
      timestamp="${BASH_REMATCH[1]}"
      body="${BASH_REMATCH[2]}"
      normalized="$(date -u -d "$timestamp" '+%Y-%m-%dT%H:%M:%S.%7NZ' 2>/dev/null || printf invalid)"
      if [ "$normalized" = "$timestamp" ] \
        && [ "$body" = "Cache not found for input keys: $DERIVED_KEY" ]; then
        clean_count=$((clean_count + 1))
      fi
    fi
  done < "$root/restore-step.log"
  [ "$clean_count" = 1 ]
}
"#;
