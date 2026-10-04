//! Fresh exact-key observer, safe step-log sampling, and result classifier.

use super::CACHE_SNAPSHOT_FUNCTION;

#[path = "schema2_mbx_cancel_probe_observer_restore_log.rs"]
mod restore_log;
use restore_log::RESTORE_LOG_HELPER;

const OBSERVER_CACHE_BEFORE_BODY: &str = r#"set -euo pipefail
root="$RUNNER_TEMP/mbx-cancel/observer"
mkdir -m 700 -p "$root"
unknown='{"count":-1,"caches":[]}'
gh_api() { gh api --hostname github.com "$@"; }
key="${VALIDATED_CACHE_KEY:-}"
case "$key" in ''|*[!a-z0-9.-]*) printf '%s\n' "$unknown" > "$root/cache-before.json"; exit 0 ;; esac
if ! gh_api --method GET "/repos/$GITHUB_REPOSITORY/actions/caches?key=$key&ref=refs/heads/main&per_page=100" \
  > "$root/cache-before-raw.json" 2>/dev/null; then
  printf '%s\n' "$unknown" > "$root/cache-before.json"
  exit 0
fi
cache_snapshot "$root/cache-before-raw.json" "$root/cache-before.json" "$key"
"#;

pub(in crate::schema2::mbx_cancel_probe) fn observer_cache_before() -> String {
    format!("{CACHE_SNAPSHOT_FUNCTION}{OBSERVER_CACHE_BEFORE_BODY}")
}

pub(in crate::schema2::mbx_cancel_probe) const OBSERVER_IMPORT_MEASURE: &str = r#"set -euo pipefail
root="$RUNNER_TEMP/mbx-cancel/observer"
mkdir -m 700 -p "$root"
count=unknown
if mbx cache stats --json > "$root/import-stats.json" 2>/dev/null; then
  count="$(jq -er '.objects | select(type == "number" and . >= 0)' "$root/import-stats.json" 2>/dev/null || printf unknown)"
fi
printf '%s\n' "$count" > "$root/import-count"
"#;

pub(in crate::schema2::mbx_cancel_probe) const OBSERVER_REUSE_MEASURE: &str = r#"set -euo pipefail
root="$RUNNER_TEMP/mbx-cancel/observer"
mkdir -m 700 -p "$root"
count=unknown
if mbx stats --json > "$root/reuse-stats.json" 2>/dev/null; then
  count="$(jq -er '.savings.cached_compilations | select(type == "number" and . >= 0)' "$root/reuse-stats.json" 2>/dev/null || printf unknown)"
fi
printf '%s\n' "$count" > "$root/reuse-count"
"#;

const OBSERVER_EVIDENCE_BODY: &str = r#"set -euo pipefail
umask 077
root="$RUNNER_TEMP/mbx-cancel/observer"
unknown='{"count":-1,"caches":[]}'
gh_api() { gh api --hostname github.com "$@"; }
test "$GITHUB_REPOSITORY" = tailrocks/velnor-new || exit 0
mkdir -m 700 -p "$root"
response="$root/save-log-response.txt"
log="$root/save-step.log"
signed_headers="$root/save-log-download.headers"
status_file="$root/save-log-api-status"
signed_config="$root/save-log.curlrc"
trap 'rm -f "$response" "$log" "$signed_headers" "$status_file" "$signed_config" "$root/save-log-api.stderr" "$root/save-log-download.stderr" "$root/restore-run.json" "$root/restore-jobs.json" "$root/restore-log-api.headers" "$root/restore-log-api.status" "$root/restore-log-api.stderr" "$root/restore-log.curlrc" "$root/restore-log-signed.headers" "$root/restore-log-signed.stderr" "$root/restore-step.log"' EXIT
upload_started=false
upload_observation=unavailable
restore_clean_miss=false
child_state=unknown/unknown
job_id=
save_status=unknown
save_conclusion=unknown
save_started=false
cancel_status=unknown
cancel_conclusion=unknown
cancel_started=false
if ! [[ "$RUN_ID" =~ ^[1-9][0-9]*$ ]]; then exit 0; fi
key="${VALIDATED_CACHE_KEY:-}"
case "$key" in ''|*[!a-z0-9.-]*) exit 0 ;; esac
if ! gh_api --method GET "/repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID" > "$root/child-final.json" 2>/dev/null; then exit 0; fi
if ! gh_api --method GET "/repos/$GITHUB_REPOSITORY/actions/runs/$RUN_ID/attempts/1/jobs?per_page=100" \
  > "$root/child-jobs-final.json" 2>/dev/null; then exit 0; fi
jq -e --argjson id "$RUN_ID" --argjson workflow "$WORKFLOW_ID" \
  --arg repo "$GITHUB_REPOSITORY" --arg sha "$CHILD_SOURCE_SHA" \
  --arg mode "$VICTIM_MODE" --arg probe "$PROBE_ID" --arg actor "$CHILD_ACTOR" \
  '.id == $id and .workflow_id == $workflow and .repository.full_name == $repo
   and .head_repository.full_name == $repo and .actor.login == $actor
   and .event == "workflow_dispatch" and .head_branch == "main" and .head_sha == $sha
   and .run_attempt == 1 and .display_title == ("MBX cancellation " + $mode + " " + $probe)
   and ((.path | split("@") | .[0]) == ".github/workflows/qualification.yml")
   and ((.path | endswith("@main")) or (.path | endswith("@refs/heads/main")))' \
  "$root/child-final.json" >/dev/null 2>&1 || exit 0
child_state="$(jq -r '(.status // "unknown") + "/" + (.conclusion // "unknown")' "$root/child-final.json")"
job_id="$(jq -er --arg name "$VICTIM_JOB_NAME" \
  '[.jobs[] | select(.name == $name)] | if length == 1 then .[0].id else error("job identity") end' \
  "$root/child-jobs-final.json" 2>/dev/null || true)"
test -n "$job_id" || exit 0
step_facts() {
  jq -c --arg job "$VICTIM_JOB_NAME" --arg step "$2" \
    '[.jobs[] | select(.name == $job)] as $jobs |
     if ($jobs | length) != 1 or ($jobs[0].steps | type) != "array" then {}
     else
       ([$jobs[0].steps[] | select(.name == $step)]) as $steps |
       if ($steps | length) == 1 then $steps[0] else {} end
     end' "$1" 2>/dev/null || printf '{}'
}
save_facts="$(step_facts "$root/child-jobs-final.json" "Save MBX single bundle")"
save_status="$(jq -r 'if (.status | type) == "string" then .status else "unknown" end' <<< "$save_facts")"
save_conclusion="$(jq -r 'if (.conclusion | type) == "string" then .conclusion else "unknown" end' <<< "$save_facts")"
save_started="$(jq -r '(.started_at | type) == "string"' <<< "$save_facts")"
cancel_facts="$(step_facts "$root/child-jobs-final.json" "$CANCEL_STEP_NAME")"
cancel_status="$(jq -r 'if (.status | type) == "string" then .status else "unknown" end' <<< "$cancel_facts")"
cancel_conclusion="$(jq -r 'if (.conclusion | type) == "string" then .conclusion else "unknown" end' <<< "$cancel_facts")"
cancel_started="$(jq -r '(.started_at | type) == "string"' <<< "$cancel_facts")"
if gh_api --method GET "/repos/$GITHUB_REPOSITORY/actions/caches?key=$key&ref=refs/heads/main&per_page=100" \
  > "$root/cache-after-raw.json" 2>/dev/null; then
  cache_snapshot "$root/cache-after-raw.json" "$root/cache-after.json" "$key"
else
  printf '%s\n' "$unknown" > "$root/cache-after.json"
fi
cancel_at="${CONTROLLER_CANCEL_AT:-}"
if [ "$PROBE_PHASE" = during-save ] && [ "$CONTROLLER_CANCEL_REQUESTED" = true ] \
  && [ "$child_state" = completed/cancelled ] && [ "$save_status" = completed ] \
  && [ "$save_conclusion" = cancelled ] && [ -n "$cancel_at" ]; then
  index="$(jq -er --arg name "$VICTIM_JOB_NAME" \
    '[.jobs[] | select(.name == $name)] as $jobs |
     if ($jobs | length) != 1 then error("job identity")
     elif ($jobs[0].steps | type) != "array" then error("steps shape")
     elif ($jobs[0].steps | length) == 0 then error("steps empty")
     else
       ($jobs[0].steps | to_entries
         | map(select(.value.name == "Save MBX single bundle"))) as $steps |
       if ($steps | length) != 1 then error("save step identity")
       else
         $steps[0].key | tonumber | select(type == "number" and . >= 0
           and . < ($jobs[0].steps | length) and . == floor)
       end
     end' \
    "$root/child-jobs-final.json" 2>/dev/null || true)"
  if [[ "$index" =~ ^[0-9]+$ ]] && [[ "$job_id" =~ ^[1-9][0-9]*$ ]]; then
    api_url="https://api.github.com/repos/tailrocks/velnor-new/actions/jobs/$job_id/steps/$index/logs"
    if [ -n "${GH_TOKEN:-}" ]; then
      (
        ulimit -f 128
        curl --disable --proto '=https' --max-redirs 0 --connect-timeout 15 --max-time 30 \
          --max-filesize 65536 --silent --show-error --fail --dump-header "$response" \
          --output /dev/null --write-out '%{http_code}' \
          --header "Authorization: Bearer $GH_TOKEN" \
          --header 'Accept: application/vnd.github+json' \
          --header 'X-GitHub-Api-Version: 2022-11-28' "$api_url" \
          > "$status_file" 2> "$root/save-log-api.stderr"
      ) || true
      status="$(cat "$status_file" 2>/dev/null || true)"
      header_size="$(wc -c < "$response" 2>/dev/null | tr -d ' ' || printf 65537)"
      location_count="$(awk 'tolower($1) == "location:" { count++ } END { print count+0 }' "$response" 2>/dev/null || printf 0)"
      if [ "$status" = 302 ] && [ "$header_size" -le 65536 ] && [ "$location_count" = 1 ]; then
        location="$(awk 'tolower($1) == "location:" { sub(/^[^:]*:[[:space:]]*/, ""); sub(/\r$/, ""); print }' "$response")"
        case "$location" in https://*) valid_location=true ;; *) valid_location=false ;; esac
        case "$location" in *[[:space:][:cntrl:]]*|*'"'*|*'\'*) valid_location=false ;; esac
        if [ "$valid_location" = true ]; then
          printf 'url = "%s"\n' "$location" > "$signed_config"
          set +o pipefail
          (
            ulimit -f 128
            curl --disable --config "$signed_config" --proto '=https' --proto-redir '=https' \
              --max-redirs 2 --connect-timeout 15 --max-time 30 --max-filesize 1048576 \
              --silent --fail --location --dump-header "$signed_headers" --output - \
              2> "$root/save-log-download.stderr"
          ) | head -c 1048577 > "$log"
          pipe_status=("${PIPESTATUS[@]}")
          set -o pipefail
          log_size="$(wc -c < "$log" | tr -d ' ' || printf 1048577)"
          signed_header_size="$(wc -c < "$signed_headers" 2>/dev/null | tr -d ' ' || printf 65537)"
          signed_status="$(awk '$1 ~ /^HTTP\// { status=$2 } END { print status }' "$signed_headers" 2>/dev/null || true)"
          if [ "${pipe_status[0]}" = 0 ] && [ "${pipe_status[1]}" = 0 ] \
            && [ "$log_size" -le 1048576 ] && [ "$signed_header_size" -le 65536 ] \
            && [ "$signed_status" = 200 ]; then
            export LC_ALL=C
canonical_decimal() {
              local value="$1"
              [[ "$value" =~ ^[0-9]{1,20}$ ]] || return 1
              value="${value#"${value%%[!0]*}"}"
              [ -n "$value" ] || value=0
              printf '%s' "$value"
            }
            decimal_less() {
              LC_ALL=C awk -v left="$1" -v right="$2" 'BEGIN {
                if (length(left) < length(right)) exit 0
                if (length(left) > length(right)) exit 1
                if (("x" left) < ("x" right)) exit 0
                exit 1
              }'
            }
            cancel_ns="$(date -u -d "$cancel_at" +%s%N 2>/dev/null || printf -1)"
            cancel_ns="$(canonical_decimal "$cancel_ns" 2>/dev/null || printf invalid)"
            # Runner source persists DateTime.UtcNow.ToString("O") plus one space.
            while IFS= read -r progress_line; do
              if [[ "$progress_line" =~ ^([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{7}Z)[[:space:]]Sent[[:space:]]([0-9]{1,20})[[:space:]]of[[:space:]]([0-9]{1,20})[[:space:]]\([0-9]+[.][0-9]%\),[[:space:]][0-9]+[.][0-9][[:space:]]MBs/sec$ ]]; then
                stamp="${BASH_REMATCH[1]}"
                sent="$(canonical_decimal "${BASH_REMATCH[2]}" 2>/dev/null || printf invalid)"
                total="$(canonical_decimal "${BASH_REMATCH[3]}" 2>/dev/null || printf invalid)"
                normalized_stamp="$(date -u -d "$stamp" '+%Y-%m-%dT%H:%M:%S.%7NZ' 2>/dev/null || printf invalid)"
                if [ "$normalized_stamp" = "$stamp" ]; then
                  upload_ns="$(date -u -d "$stamp" +%s%N 2>/dev/null || printf -1)"
                else
                  upload_ns=invalid
                fi
                upload_ns="$(canonical_decimal "$upload_ns" 2>/dev/null || printf invalid)"
                if [[ "$sent" =~ ^[1-9][0-9]*$ ]] && [[ "$total" =~ ^[1-9][0-9]*$ ]] \
                  && [[ "$upload_ns" =~ ^[0-9]+$ ]] && [[ "$cancel_ns" =~ ^[0-9]+$ ]] \
                  && decimal_less "$sent" "$total" && decimal_less "$upload_ns" "$cancel_ns"; then
                  upload_started=true
                  upload_observation=timestamped_positive_partial_progress_before_cancel
                  break
                fi
              fi
            done < "$log"
          fi
        fi
      fi
    fi
  fi
fi
before='{"count":-1,"caches":[]}'
if [ -s "$root/cache-before.json" ]; then before="$(jq -c . "$root/cache-before.json")"; fi
after="$unknown"
if [ -s "$root/cache-after.json" ]; then after="$(jq -c . "$root/cache-after.json")"; fi
if [ "${RESTORE_CONCLUSION:-}" = success ] \
  && [ "${RESTORE_PRIMARY_KEY:-}" = "${DERIVED_KEY:-}" ] \
  && fetch_restore_log; then
  restore_clean_miss=true
fi
jq -cn --arg run "$RUN_ID" --arg child_state "$child_state" --arg job_id "$job_id" \
  --arg save_status "$save_status" --arg save_conclusion "$save_conclusion" \
  --argjson save_started "$save_started" --arg cancel_status "$cancel_status" \
  --arg cancel_conclusion "$cancel_conclusion" --argjson cancel_started "$cancel_started" \
  --argjson upload_started "$upload_started" --arg upload_observation "$upload_observation" \
  --argjson restore_clean_miss "$restore_clean_miss" \
  --argjson cache_after "$after" --argjson cache_before "$before" \
  --argjson controller_before_count "${CONTROLLER_BEFORE_COUNT:--1}" \
  '{child_run_id:$run,child_state:$child_state,child_job_id:$job_id,
    save_step_status:$save_status,save_step_conclusion:$save_conclusion,save_step_started:$save_started,
    cancel_step_status:$cancel_status,cancel_step_conclusion:$cancel_conclusion,
    cancel_step_started:$cancel_started,upload_started_before_cancel:$upload_started,
    upload_observation:$upload_observation,restore_clean_miss:$restore_clean_miss,
    controller_cache_before_count:$controller_before_count,
    cache_before:$cache_before,cache_after:$cache_after}' \
  > "$root/child-evidence.json"
"#;

pub(in crate::schema2::mbx_cancel_probe) fn observer_evidence() -> String {
    format!("{CACHE_SNAPSHOT_FUNCTION}{RESTORE_LOG_HELPER}{OBSERVER_EVIDENCE_BODY}")
}

pub(in crate::schema2::mbx_cancel_probe) const OBSERVER_CLASSIFY: &str = r#"set -euo pipefail
root="$RUNNER_TEMP/mbx-cancel/observer"
result="$root/result.json"
outcome=NOT_RUN
reason=exact_identity_or_observation_missing
cache_state=UNKNOWN
restore_state=UNKNOWN
restore_evidence=false
restore_clean_miss=false
import_count=unknown
reuse_count=unknown
before_count=-1
after_count=-1
upload_started=false
should_observe=false
if [ "${SHOULD_OBSERVE:-}" = true ]; then should_observe=true; fi
controller_before_count="${CONTROLLER_BEFORE_COUNT:--1}"
if ! [[ "$controller_before_count" =~ ^(-1|0|[1-9][0-9]{0,19})$ ]]; then
  controller_before_count=-1
fi
if [ -s "$root/import-count" ]; then IFS= read -r import_count < "$root/import-count"; fi
if [ -s "$root/reuse-count" ]; then IFS= read -r reuse_count < "$root/reuse-count"; fi
if [ -s "$root/child-evidence.json" ]; then
  before_count="$(jq -r '.cache_before.count' "$root/child-evidence.json")"
  after_count="$(jq -r '.cache_after.count' "$root/child-evidence.json")"
  upload_started="$(jq -r '.upload_started_before_cancel' "$root/child-evidence.json")"
  restore_clean_miss="$(jq -r 'if .restore_clean_miss == true then "true" else "false" end' "$root/child-evidence.json")"
fi
if [ "$after_count" = 0 ]; then cache_state=MISS; elif [ "$after_count" = 1 ]; then cache_state=HIT; fi
restore_hit="${RESTORE_HIT:-}"
matched_key="${MATCHED_KEY:-}"
restore_primary="${RESTORE_PRIMARY_KEY:-}"
restore_conclusion="${RESTORE_CONCLUSION:-}"
derived_key="${DERIVED_KEY:-}"
validated_key="${VALIDATED_CACHE_KEY:-}"
case "$restore_hit" in
  true)
    if [ "$restore_conclusion" = success ] && [ "$restore_primary" = "$derived_key" ] \
      && [ -n "$matched_key" ] && [ "$matched_key" = "$derived_key" ]; then
      restore_state=HIT
      restore_evidence=true
    fi
    ;;
  '')
    if [ "$restore_conclusion" = success ] && [ "$restore_primary" = "$derived_key" ] \
      && [ -z "$matched_key" ] && [ "$restore_clean_miss" = true ]; then
      restore_state=MISS
      restore_evidence=true
    fi
    ;;
esac
if [ "$should_observe" = true ] && [ -s "$root/child-evidence.json" ] \
  && [ -n "$derived_key" ] && [ "$derived_key" = "$validated_key" ]; then
  if [ "$restore_evidence" != true ]; then
    if [ -z "$restore_hit" ] && [ "$restore_conclusion" = success ] \
      && [ "$restore_primary" = "$derived_key" ] && [ -z "$matched_key" ]; then
      reason=restore_miss_log_or_cache_api_not_proven
    else
      reason=restore_action_outputs_missing_or_inconsistent
    fi
  else
    cancel_ok=false
    if [ "$CONTROLLER_READY" = true ] \
      && [ "$CONTROLLER_READY_REASON" = exact_identity_and_cancel_window_ready ] \
      && [ "$CONTROLLER_CANCEL_REQUESTED" = true ] \
      && [ "$CONTROLLER_CANCEL_STATUS" = 202 ] \
      && [ "$CONTROLLER_POST_REVALIDATED" = true ]; then cancel_ok=true; fi
    terminal_ok=false
    if [ "$CONTROLLER_TERMINAL" = true ] \
      && [ "$CONTROLLER_TERMINAL_STATE" = completed/cancelled ]; then terminal_ok=true; fi
    if [ "$PROBE_PHASE" = pre-save ]; then
      never_saved="$(jq -r '.save_step_started == false and .save_step_conclusion == "skipped"' "$root/child-evidence.json")"
      wait_cancelled="$(jq -r '.cancel_step_conclusion == "cancelled"' "$root/child-evidence.json")"
      if [ "$cancel_ok" = true ] && [ "$terminal_ok" = true ] && [ "$never_saved" = true ] \
        && [ "$wait_cancelled" = true ] && [ "$before_count" = 0 ] && [ "$after_count" = 0 ] \
        && [ "$controller_before_count" = 0 ] \
        && [ "$restore_state" = MISS ]; then
        outcome=MUSTMISS
        reason=exact_pre_save_cancellation_with_cold_fresh_restore
      fi
    elif [ "$PROBE_PHASE" = during-save ]; then
      window_ok="$(jq -r '.save_step_started == true and .save_step_conclusion == "cancelled"' "$root/child-evidence.json")"
      if [ "$cancel_ok" = true ] && [ "$terminal_ok" = true ] && [ "$window_ok" = true ] \
        && [ "$upload_started" = true ]; then
        if [ "$before_count" = -1 ] || [ "$after_count" = -1 ] \
          || [ "$controller_before_count" = -1 ]; then
          reason=exact_cache_api_snapshot_unavailable
        elif [ "$after_count" = 1 ] && [ "$restore_state" = HIT ] \
          && [ "$before_count" = 0 ] && [ "$controller_before_count" = 0 ] \
          && [[ "$import_count" =~ ^[1-9][0-9]*$ ]] && [[ "$reuse_count" =~ ^[1-9][0-9]*$ ]]; then
          outcome=HIT
          reason=exact_key_restored_imported_and_reused_after_cancelled_upload
        elif [ "$after_count" = 0 ] && [ "$restore_state" = MISS ] \
          && [ "$controller_before_count" = 0 ] && [ "$before_count" = 0 ]; then
          outcome=RESERVATION-UNKNOWN/INCONCLUSIVE
          reason=upload_started_before_cancel_but_no_committed_exact_cache_observed
        else
          outcome=INCONCLUSIVE
          reason=cache_api_restore_or_import_evidence_disagreed
        fi
      else
        reason=save_upload_window_not_proven_before_cancel
      fi
    fi
  fi
fi
mkdir -m 700 -p "$root"
victim=null
controller=null
evidence=null
if [ -s "$root/child-evidence.json" ]; then evidence="$(jq -c . "$root/child-evidence.json")"; fi
if [ -s "$root/validated-victim.json" ]; then victim="$(jq -c . "$root/validated-victim.json")"; fi
jq -cn --arg probe "$PROBE_ID" --arg mode "$CONTROLLER_MODE" \
  --arg ready "$CONTROLLER_READY" --arg ready_reason "$CONTROLLER_READY_REASON" \
  --arg cancel_requested "$CONTROLLER_CANCEL_REQUESTED" \
  --arg cancel_status "$CONTROLLER_CANCEL_STATUS" \
  --arg post_revalidated "$CONTROLLER_POST_REVALIDATED" \
  --arg cancel_at "$CONTROLLER_CANCEL_AT" --arg terminal "$CONTROLLER_TERMINAL" \
  --arg terminal_state "$CONTROLLER_TERMINAL_STATE" \
  --argjson before_count "$controller_before_count" \
  '{probe_id:$probe,mode:$mode,ready:($ready == "true"),ready_reason:$ready_reason,
    cancel_requested:($cancel_requested == "true"),cancel_status:$cancel_status,
    post_revalidated:($post_revalidated == "true"),cancel_request_started_at:$cancel_at,
    terminal:($terminal == "true"),terminal_state:$terminal_state,
    cache_before_count:$before_count}' > "$root/controller-summary.json"
controller="$(jq -c . "$root/controller-summary.json")"
jq -cn --arg outcome "$outcome" --arg reason "$reason" --arg cache_state "$cache_state" \
  --arg restore_state "$restore_state" --arg import_count "$import_count" --arg reuse_count "$reuse_count" \
  --arg derived_key "$derived_key" --arg scope "$CACHE_SCOPE" --arg version "$MBX_VERSION" \
  --arg generation "$GENERATION" --argjson should_observe "$should_observe" \
  --argjson controller_before_count "$controller_before_count" \
  --argjson controller "$controller" --argjson victim "$victim" --argjson evidence "$evidence" \
  '{schema:1,outcome:$outcome,reason:$reason,should_observe:$should_observe,
    exact_cache_state:$cache_state,restore_state:$restore_state,cache_scope:$scope,
    mbx_version:$version,generation:$generation,derived_primary:$derived_key,
    controller_cache_before_count:$controller_before_count,
    imported_objects:$import_count,cached_compilations_reused:$reuse_count,
    controller:$controller,victim:$victim,child_evidence:$evidence}' > "$result"
printf 'MBX cancellation probe: %s (%s)\n' "$outcome" "$reason" >> "$GITHUB_STEP_SUMMARY"
"#;
