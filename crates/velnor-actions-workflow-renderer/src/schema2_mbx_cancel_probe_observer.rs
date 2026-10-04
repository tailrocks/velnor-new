//! Fresh exact-key observer, safe step-log sampling, and result classifier.

use super::CACHE_SNAPSHOT_FUNCTION;

#[path = "schema2_mbx_cancel_probe_observer_restore_log.rs"]
mod restore_log;
use restore_log::RESTORE_LOG_HELPER;

const OBSERVER_CACHE_BEFORE_BODY: &str = r#"set -euo pipefail
root="$RUNNER_TEMP/mbx-cancel-observer/observer"
private_storage_open "$root" || exit 0
unknown='{"count":-1,"caches":[]}'
key="${VALIDATED_CACHE_KEY:-}"
case "$key" in ''|*[!a-z0-9.-]*) private_capture "$root" "$root/cache-before.json" 65536 printf '%s\n' "$unknown"; exit 0 ;; esac
if ! private_api_json "$root" "$root/cache-before-raw.json" \
  "/repos/$GITHUB_REPOSITORY/actions/caches?key=$key&ref=refs/heads/main&per_page=100" 2>/dev/null \
  || ! private_list_complete "$root/cache-before-raw.json" actions_caches; then
  private_capture "$root" "$root/cache-before.json" 65536 printf '%s\n' "$unknown"
  exit 0
fi
private_capture "$root" "$root/cache-before.json" 65536 \
  cache_snapshot "$root/cache-before-raw.json" "$key"
"#;

pub(in crate::schema2::mbx_cancel_probe) fn observer_cache_before() -> String {
    format!("{CACHE_SNAPSHOT_FUNCTION}{OBSERVER_CACHE_BEFORE_BODY}")
}

pub(in crate::schema2::mbx_cancel_probe) const OBSERVER_IMPORT_MEASURE: &str = r#"set -euo pipefail
root="$RUNNER_TEMP/mbx-cancel-observer/observer"
private_storage_open "$root" || exit 0
count=unknown
if private_capture "$root" "$root/import-stats.json" 65536 mbx cache stats --json 2>/dev/null \
  && private_json_valid "$root" "$root/import-stats.json" 65536; then
  count="$(jq -er '.objects | select(type == "number" and . >= 0)' "$root/import-stats.json" 2>/dev/null || printf unknown)"
fi
private_capture "$root" "$root/import-count" 128 printf '%s\n' "$count"
"#;

pub(in crate::schema2::mbx_cancel_probe) const OBSERVER_REUSE_MEASURE: &str = r#"set -euo pipefail
root="$RUNNER_TEMP/mbx-cancel-observer/observer"
private_storage_open "$root" || exit 0
count=unknown
if private_capture "$root" "$root/reuse-stats.json" 65536 mbx stats --json 2>/dev/null \
  && private_json_valid "$root" "$root/reuse-stats.json" 65536; then
  count="$(jq -er '.savings.cached_compilations | select(type == "number" and . >= 0)' "$root/reuse-stats.json" 2>/dev/null || printf unknown)"
fi
private_capture "$root" "$root/reuse-count" 128 printf '%s\n' "$count"
"#;

const OBSERVER_EVIDENCE_BODY: &str = r#"set -euo pipefail
umask 077
root="$RUNNER_TEMP/mbx-cancel-observer/observer"
private_storage_open "$root" || exit 0
unknown='{"count":-1,"caches":[]}'
test "$GITHUB_REPOSITORY" = tailrocks/velnor-new || exit 0
controller_cancel_status="${CONTROLLER_CANCEL_STATUS:-}"
controller_cancel_requested="${CONTROLLER_CANCEL_REQUESTED:-false}"
controller_post_revalidated="${CONTROLLER_POST_REVALIDATED:-false}"
RESTORE_LOG_ROOT="$RUNNER_TEMP/mbx-cancel-restore-parent"
SAVE_LOG_ROOT="$RUNNER_TEMP/mbx-cancel-restore-save"
trap 'private_root_remove "$RESTORE_LOG_ROOT" || true; private_root_remove "$SAVE_LOG_ROOT" || true' EXIT
progress_before_runner_cancel_error=false
progress_observation=not_proven
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
if ! stock_restore_api_snapshot "$SAVE_LOG_ROOT" "$RUN_ID" 1 "$CHILD_SOURCE_SHA" 2>/dev/null; then exit 0; fi
child_run_document="$(stock_restore_single_object "$SAVE_LOG_ROOT/run.json" 2097152)" || exit 0
child_jobs_document="$(stock_restore_single_object "$SAVE_LOG_ROOT/jobs.json" 2097152)" || exit 0
jq -e --argjson id "$RUN_ID" --argjson workflow "$WORKFLOW_ID" \
  --arg repo "$GITHUB_REPOSITORY" --arg sha "$CHILD_SOURCE_SHA" \
  --arg mode "$VICTIM_MODE" --arg probe "$PROBE_ID" --arg actor "$CHILD_ACTOR" \
  '.id == $id and .workflow_id == $workflow and .repository.full_name == $repo
   and .head_repository.full_name == $repo and .actor.login == $actor
   and .event == "workflow_dispatch" and .head_branch == "main" and .head_sha == $sha
   and .run_attempt == 1 and .display_title == ("MBX cancellation " + $mode + " " + $probe)
   and ((.path | split("@") | .[0]) == ".github/workflows/qualification.yml")
   and ((.path | endswith("@main")) or (.path | endswith("@refs/heads/main")))' \
  <<< "$child_run_document" >/dev/null 2>&1 || exit 0
child_state="$(jq -r '(.status // "unknown") + "/" + (.conclusion // "unknown")' <<< "$child_run_document")"
job_id="$(jq -er --arg name "$VICTIM_JOB_NAME" \
  '[.jobs[] | select(.name == $name)] | if length == 1 then .[0].id else error("job identity") end' \
  <<< "$child_jobs_document" 2>/dev/null || true)"
test -n "$job_id" || exit 0
step_facts() {
  jq -c --arg job "$VICTIM_JOB_NAME" --arg step "$1" \
    '[.jobs[] | select(.name == $job)] as $jobs |
     if ($jobs | length) != 1 or ($jobs[0].steps | type) != "array" then {}
     else
       ([$jobs[0].steps[] | select(.name == $step)]) as $steps |
       if ($steps | length) == 1 then $steps[0] else {} end
     end' <<< "$child_jobs_document" 2>/dev/null || printf '{}'
}
save_facts="$(step_facts "Save MBX single bundle")"
save_status="$(jq -r 'if (.status | type) == "string" then .status else "unknown" end' <<< "$save_facts")"
save_conclusion="$(jq -r 'if (.conclusion | type) == "string" then .conclusion else "unknown" end' <<< "$save_facts")"
save_started="$(jq -r '(.started_at | type) == "string"' <<< "$save_facts")"
cancel_facts="$(step_facts "$CANCEL_STEP_NAME")"
cancel_status="$(jq -r 'if (.status | type) == "string" then .status else "unknown" end' <<< "$cancel_facts")"
cancel_conclusion="$(jq -r 'if (.conclusion | type) == "string" then .conclusion else "unknown" end' <<< "$cancel_facts")"
cancel_started="$(jq -r '(.started_at | type) == "string"' <<< "$cancel_facts")"
if private_api_json "$root" "$root/cache-after-raw.json" \
  "/repos/$GITHUB_REPOSITORY/actions/caches?key=$key&ref=refs/heads/main&per_page=100" 2>/dev/null \
  && private_list_complete "$root/cache-after-raw.json" actions_caches; then
  private_capture "$root" "$root/cache-after.json" 65536 \
    cache_snapshot "$root/cache-after-raw.json" "$key"
else
  private_capture "$root" "$root/cache-after.json" 65536 printf '%s\n' "$unknown"
fi
if [ "$PROBE_PHASE" = during-save ] && [ "$controller_cancel_requested" = true ] \
  && [ "$controller_cancel_status" = 202 ] && [ "$controller_post_revalidated" = true ] \
  && [ "$child_state" = completed/cancelled ] && [ "$save_status" = completed ] \
  && [ "$save_conclusion" = cancelled ] && [ "$save_started" = true ]; then
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
    <<< "$child_jobs_document" 2>/dev/null || true)"
  if [[ "$index" =~ ^[0-9]+$ ]] && [[ "$job_id" =~ ^[1-9][0-9]*$ ]]; then
    if fetch_save_step_log "$job_id" "$index" \
      && save_log_proves_partial_before_cancel_error "$SAVE_LOG_ROOT/restore-step.log"; then
      progress_before_runner_cancel_error=true
      progress_observation=positive_partial_sdk_progress_precedes_runner_cancel_error
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
private_capture "$root" "$root/child-evidence.json" 65536 jq -cn \
  --arg run "$RUN_ID" --arg child_state "$child_state" --arg job_id "$job_id" \
  --arg save_status "$save_status" --arg save_conclusion "$save_conclusion" \
  --argjson save_started "$save_started" --arg cancel_status "$cancel_status" \
  --arg cancel_conclusion "$cancel_conclusion" --argjson cancel_started "$cancel_started" \
  --argjson progress_precedes_runner_error "$progress_before_runner_cancel_error" \
  --arg progress_observation "$progress_observation" \
  --argjson restore_clean_miss "$restore_clean_miss" \
  --argjson cache_after "$after" --argjson cache_before "$before" \
  --argjson controller_before_count "${CONTROLLER_BEFORE_COUNT:--1}" \
  '{child_run_id:$run,child_state:$child_state,child_job_id:$job_id,
    save_step_status:$save_status,save_step_conclusion:$save_conclusion,save_step_started:$save_started,
    cancel_step_status:$cancel_status,cancel_step_conclusion:$cancel_conclusion,
    cancel_step_started:$cancel_started,
    progress_before_runner_cancel_error:$progress_precedes_runner_error,
    progress_observation:$progress_observation,restore_clean_miss:$restore_clean_miss,
    controller_cache_before_count:$controller_before_count,
    cache_before:$cache_before,cache_after:$cache_after}'
"#;

pub(in crate::schema2::mbx_cancel_probe) fn observer_evidence() -> String {
    format!("{CACHE_SNAPSHOT_FUNCTION}{RESTORE_LOG_HELPER}{OBSERVER_EVIDENCE_BODY}")
}

pub(in crate::schema2::mbx_cancel_probe) const OBSERVER_CLASSIFY: &str = r#"set -euo pipefail
root="$RUNNER_TEMP/mbx-cancel-observer/observer"
private_storage_open "$root" || exit 0
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
progress_before_runner_cancel_error=false
should_observe=false
controller_cancel_status="${CONTROLLER_CANCEL_STATUS:-}"
if [ "${SHOULD_OBSERVE:-}" = true ]; then should_observe=true; fi
controller_before_count="${CONTROLLER_BEFORE_COUNT:--1}"
if ! [[ "$controller_before_count" =~ ^(-1|0|[1-9][0-9]{0,19})$ ]]; then
  controller_before_count=-1
fi
if private_file_valid "$root" "$root/import-count" 128; then IFS= read -r import_count < "$root/import-count"; fi
if private_file_valid "$root" "$root/reuse-count" 128; then IFS= read -r reuse_count < "$root/reuse-count"; fi
if private_json_valid "$root" "$root/child-evidence.json" 65536; then
  before_count="$(jq -r '.cache_before.count' "$root/child-evidence.json")"
  after_count="$(jq -r '.cache_after.count' "$root/child-evidence.json")"
  progress_before_runner_cancel_error="$(jq -r '.progress_before_runner_cancel_error' "$root/child-evidence.json")"
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
if [ "$should_observe" = true ] && private_json_valid "$root" "$root/child-evidence.json" 65536 \
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
    if [ "${CONTROLLER_READY:-}" = true ] \
      && [ "${CONTROLLER_READY_REASON:-}" = exact_identity_and_cancel_window_ready ] \
      && [ "${CONTROLLER_CANCEL_REQUESTED:-false}" = true ] \
      && [ "$controller_cancel_status" = 202 ] \
      && [ "${CONTROLLER_POST_REVALIDATED:-false}" = true ]; then cancel_ok=true; fi
    terminal_ok=false
    if [ "${CONTROLLER_TERMINAL:-false}" = true ] \
      && [ "${CONTROLLER_TERMINAL_STATE:-}" = completed/cancelled ]; then terminal_ok=true; fi
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
      && [ "$progress_before_runner_cancel_error" = true ]; then
        if [ "$before_count" = -1 ] || [ "$after_count" = -1 ] \
          || [ "$controller_before_count" = -1 ]; then
          reason=exact_cache_api_snapshot_unavailable
        elif [ "$after_count" = 1 ] && [ "$restore_state" = HIT ] \
          && [[ "$before_count" =~ ^[01]$ ]] && [ "$controller_before_count" = 0 ] \
          && [[ "$import_count" =~ ^[1-9][0-9]*$ ]] && [[ "$reuse_count" =~ ^[1-9][0-9]*$ ]]; then
          outcome=HIT
          reason=exact_key_restored_imported_and_reused_after_cancelled_save
        elif [ "$after_count" = 0 ] && [ "$restore_state" = MISS ] \
          && [ "$controller_before_count" = 0 ] && [ "$before_count" = 0 ]; then
          outcome=MISS
          reason=cancelled_save_clean_exact_key_absence_reservation_unknown
        else
          outcome=INCONCLUSIVE
          reason=cache_api_restore_or_import_evidence_disagreed
        fi
      else
        reason=cancelled_save_progress_order_or_runner_error_not_proven
      fi
    fi
  fi
fi
victim=null
controller=null
evidence=null
if private_json_valid "$root" "$root/child-evidence.json" 65536; then
  evidence="$(jq -c . "$root/child-evidence.json")"
fi
if private_json_valid "$root" "$root/validated-victim.json" 65536; then
  victim="$(jq -c . "$root/validated-victim.json")"
fi
private_capture "$root" "$root/controller-summary.json" 65536 jq -cn \
  --arg probe "$PROBE_ID" --arg mode "$CONTROLLER_MODE" \
  --arg ready "${CONTROLLER_READY:-}" --arg ready_reason "${CONTROLLER_READY_REASON:-}" \
  --arg cancel_requested "${CONTROLLER_CANCEL_REQUESTED:-false}" \
  --arg cancel_status "$controller_cancel_status" \
  --arg post_revalidated "${CONTROLLER_POST_REVALIDATED:-false}" \
  --arg cancel_at "${CONTROLLER_CANCEL_AT:-}" --arg terminal "${CONTROLLER_TERMINAL:-false}" \
  --arg terminal_state "${CONTROLLER_TERMINAL_STATE:-unknown}" \
  --argjson before_count "$controller_before_count" \
  '{probe_id:$probe,mode:$mode,ready:($ready == "true"),ready_reason:$ready_reason,
    cancel_requested:($cancel_requested == "true"),cancel_status:$cancel_status,
    post_revalidated:($post_revalidated == "true"),cancel_request_started_at:$cancel_at,
    terminal:($terminal == "true"),terminal_state:$terminal_state,
    cache_before_count:$before_count}'
controller="$(jq -c . "$root/controller-summary.json")"
private_capture "$root" "$result" 65536 jq -cn \
  --arg outcome "$outcome" --arg reason "$reason" --arg cache_state "$cache_state" \
  --arg restore_state "$restore_state" --arg import_count "$import_count" --arg reuse_count "$reuse_count" \
  --arg derived_key "$derived_key" --arg scope "$CACHE_SCOPE" --arg version "$MBX_VERSION" \
  --arg generation "$GENERATION" --argjson should_observe "$should_observe" \
  --argjson controller_before_count "$controller_before_count" \
  --arg reservation_state "UNKNOWN" \
  --argjson controller "$controller" --argjson victim "$victim" --argjson evidence "$evidence" \
  '{schema:1,outcome:$outcome,reason:$reason,should_observe:$should_observe,
    exact_cache_state:$cache_state,restore_state:$restore_state,cache_scope:$scope,
    mbx_version:$version,generation:$generation,derived_primary:$derived_key,
    controller_cache_before_count:$controller_before_count,
    reservation_state:$reservation_state,
    imported_objects:$import_count,cached_compilations_reused:$reuse_count,
    controller:$controller,victim:$victim,child_evidence:$evidence}'
printf 'MBX cancellation probe: %s (%s)\n' "$outcome" "$reason" >> "$GITHUB_STEP_SUMMARY"
"#;
