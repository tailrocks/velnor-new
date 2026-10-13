//! Fake commands for cancellation shell fixtures.

pub(super) const FAKE_GH: &str = r#"#!/usr/bin/env bash
set -euo pipefail
method=GET
include=false
input=
output=
endpoint=
hostname=
while (($#)); do
  case "$1" in
    --method) method="$2"; shift 2 ;;
    --input) input="$2"; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    --include) include=true; shift ;;
    --hostname) hostname="$2"; shift 2 ;;
    *) endpoint="$1"; shift ;;
  esac
done
test "$hostname" = github.com
printf '%s %s\n' "$method" "$endpoint" >> "$GH_LOG"
test "$method" = POST
case "$method:$endpoint" in
  GET:/repos/tailrocks/velnor-new/actions/workflows/qualification.yml)
    printf '%s\n' '{"id":77,"path":".github/workflows/qualification.yml","state":"active"}' ;;
  POST:/repos/tailrocks/velnor-new/actions/workflows/qualification.yml/dispatches)
    jq -e --arg mode "$VICTIM_MODE" --arg probe "$PROBE_ID" \
      '.ref == "refs/heads/main" and .return_run_details == true
       and .inputs.mode == $mode and .inputs.probe_id == $probe' "$input" >/dev/null
    if [ "${GH_MODE:-good}" = missing-id ]; then
      printf 'HTTP/2 200 OK\r\n\r\n%s\n' \
        '{"run_url":"https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123"}'
    elif [ "${GH_MODE:-good}" = bad-url ]; then
      printf 'HTTP/2 200 OK\r\n\r\n%s\n' \
        '{"workflow_run_id":123,"run_url":"https://api.github.com/repos/other/repo/actions/runs/123"}'
    else
      printf 'HTTP/2 200 OK\r\n\r\n%s\n' \
        '{"workflow_run_id":123,"run_url":"https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123"}'
    fi ;;
  GET:/repos/tailrocks/velnor-new/actions/runs/900)
    controller_mode="$(jq -er '.inputs.mode' "$GITHUB_EVENT_PATH")"
    controller_probe="$(jq -er '.inputs.probe_id' "$GITHUB_EVENT_PATH")"
    test "$controller_mode" = "$CONTROLLER_MODE"
    test "$controller_probe" != "$PROBE_ID"
    controller_title="MBX cancellation $controller_mode"
    if [ -n "$controller_probe" ]; then controller_title="$controller_title $controller_probe"; fi
    jq -cn --arg sha "$GITHUB_SHA" --arg actor "$GITHUB_ACTOR" \
      --arg title "$controller_title" \
      --argjson workflow "${OBSERVER_WORKFLOW_ID:-77}" \
      '{id:900,workflow_id:$workflow,path:".github/workflows/qualification.yml@main",
        repository:{full_name:"tailrocks/velnor-new"},head_repository:{full_name:"tailrocks/velnor-new"},
        event:"workflow_dispatch",head_branch:"main",head_sha:$sha,run_attempt:1,
        display_title:$title,actor:{login:$actor}}' ;;
  GET:/repos/tailrocks/velnor-new/actions/runs/900/attempts/1/jobs?per_page=100)
    jq -cn --arg name "$OBSERVER_JOB_NAME" \
      '{total_count:1,jobs:[{id:901,name:$name,status:"in_progress",steps:[
        {name:"Set up job",status:"completed",conclusion:"success",number:1},
        {name:"Prepare MBX bundle key",status:"completed",conclusion:"success"},
        {name:"Restore MBX single bundle",status:"completed",conclusion:"success",number:13}]}]}' ;;
  GET:/repos/tailrocks/velnor-new/actions/runs/123)
    if [ "$GH_MODE" = mismatch ]; then repo=attacker/repo; else repo=tailrocks/velnor-new; fi
    run_count=0
    if [ -s "$GH_STATE.run-count" ]; then IFS= read -r run_count < "$GH_STATE.run-count"; fi
    run_count=$((run_count + 1))
    printf '%s\n' "$run_count" > "$GH_STATE.run-count"
    actor=github-actions[bot]
    run_sha="$GITHUB_SHA"
    attempt=1
    if [ "$GH_MODE" = initial-mismatch ] && [ "$run_count" = 1 ]; then
      actor=unexpected-actor
      run_sha=dddddddddddddddddddddddddddddddddddddddd
    fi
    if [ "$GH_MODE" = revalidate-actor ] && [ "$run_count" -ge 5 ]; then actor=unexpected-actor; fi
    if [ "$GH_MODE" = revalidate-attempt ] && [ "$run_count" -ge 5 ]; then attempt=2; fi
    status=in_progress
    conclusion=null
    case "$GH_MODE" in
      observer-*|terminal-good) status=completed; conclusion='"cancelled"' ;;
      terminal-invalid-status) status=$'bad\ninjected=true' ;;
      terminal-unknown-status) status=unexpected ;;
      terminal-invalid-conclusion)
        status=completed
        conclusion="$(jq -cn --arg value $'bad\ninjected=true' '$value')" ;;
      terminal-unknown-conclusion) status=completed; conclusion='"unexpected"' ;;
      terminal-null-conclusion) status=completed ;;
    esac
    jq -cn --arg repo "$repo" --arg sha "$run_sha" --arg mode "$VICTIM_MODE" \
      --arg probe "$PROBE_ID" --arg status "$status" --argjson conclusion "$conclusion" \
      --arg actor "$actor" --argjson attempt "$attempt" \
      '{id:123,workflow_id:77,path:".github/workflows/qualification.yml@main",
        repository:{full_name:$repo},head_repository:{full_name:"tailrocks/velnor-new"},
        event:"workflow_dispatch",head_branch:"main",head_sha:$sha,run_attempt:$attempt,
        display_title:("MBX cancellation " + $mode + " " + $probe),
        actor:{login:$actor},status:$status,conclusion:$conclusion}' ;;
  GET:/repos/tailrocks/velnor-new/actions/runs/123/attempts/1/jobs?per_page=100)
    case "${GH_MODE:-good}" in
    observer-window)
      jq -cn --arg name "$VICTIM_JOB_NAME" \
        '{total_count:1,jobs:[{id:456,name:$name,status:"completed",steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
          {name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:13,started_at:"2026-10-04T00:00:00Z"}]}]}'
      ;;
    observer-duplicate-save)
      jq -cn --arg name "$VICTIM_JOB_NAME" \
        '{total_count:1,jobs:[{id:456,name:$name,status:"completed",steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
          {name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:13,started_at:"2026-10-04T00:00:00Z"},
          {name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:14,started_at:"2026-10-04T00:00:00Z"}]}]}'
      ;;
    observer-missing-save)
      jq -cn --arg name "$VICTIM_JOB_NAME" \
        '{total_count:1,jobs:[{id:456,name:$name,status:"completed",steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"}]}]}'
      ;;
    observer-object-steps)
      jq -cn --arg name "$VICTIM_JOB_NAME" \
        '{total_count:1,jobs:[{id:456,name:$name,status:"completed",steps:{save:{
          name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:13,started_at:"2026-10-04T00:00:00Z"}}}]}'
      ;;
    *)
      if [ "$PROBE_PHASE" = pre-save ]; then
        jq -cn --arg name "$VICTIM_JOB_NAME" \
          '{total_count:1,jobs:[{id:456,name:$name,status:"in_progress",steps:[
            {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
            {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
            {name:"Wait at MBX pre-save cancellation point",status:"in_progress",conclusion:null},
            {name:"Save MBX single bundle",status:"completed",conclusion:"skipped"}]}]}'
        exit 0
      fi
      jobs_count=0
      if [ -s "$GH_STATE.jobs-count" ]; then IFS= read -r jobs_count < "$GH_STATE.jobs-count"; fi
      jobs_count=$((jobs_count + 1))
      printf '%s\n' "$jobs_count" > "$GH_STATE.jobs-count"
      job_status=in_progress
      save_status=in_progress
      save_conclusion=null
      if [ "$GH_MODE" = revalidate-job ] && [ "$jobs_count" -ge 3 ]; then
        job_status=completed
        save_status=completed
        save_conclusion=cancelled
      fi
      jq -cn --arg name "$VICTIM_JOB_NAME" \
        --arg job_status "$job_status" --arg save_status "$save_status" \
        --argjson save_conclusion "$save_conclusion" \
        '{total_count:1,jobs:[{id:456,name:$name,status:$job_status,steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
          {name:"Save MBX single bundle",status:$save_status,conclusion:$save_conclusion,number:13}]}]}'
      ;;
    esac ;;
  GET:/repos/tailrocks/velnor-new/actions/runs/123/artifacts?per_page=100)
    artifacts_count=0
    if [ -s "$GH_STATE.artifacts-count" ]; then IFS= read -r artifacts_count < "$GH_STATE.artifacts-count"; fi
    artifacts_count=$((artifacts_count + 1))
    printf '%s\n' "$artifacts_count" > "$GH_STATE.artifacts-count"
    artifact_id=55
    if [ "$GH_MODE" = revalidate-artifact ] && [ "$artifacts_count" -ge 2 ]; then artifact_id=56; fi
    jq -cn --arg name "$VICTIM_ARTIFACT_NAME" --arg digest "$GH_ARTIFACT_DIGEST" \
      --argjson artifact_id "$artifact_id" \
      --argjson size "$GH_ARTIFACT_SIZE" \
      '{total_count:1,artifacts:[{id:$artifact_id,name:$name,expired:false,size_in_bytes:$size,
        digest:("sha256:" + $digest),workflow_run:{id:123}}]}' ;;
  GET:/repos/tailrocks/velnor-new/actions/artifacts/55/zip)
    test -n "$output"
    cp "$GH_ARTIFACT_ZIP" "$output" ;;
  GET:/repos/tailrocks/velnor-new/actions/caches?key=*)
    case "${GH_MODE:-good}" in
      cache-object) printf '%s\n' '{"total_count":0,"actions_caches":{}}' ;;
      cache-null-response) printf '%s\n' 'null' ;;
      cache-null-entry) printf '%s\n' '{"total_count":1,"actions_caches":[null]}' ;;
      cache-missing-array) printf '%s\n' '{"count":0,"caches":[]}' ;;
      cache-missing-total) printf '%s\n' '{"actions_caches":[]}' ;;
      cache-count-mismatch) printf '%s\n' '{"total_count":1,"actions_caches":[]}' ;;
      cache-truncated-page) printf '%s\n' '{"total_count":101,"actions_caches":[]}' ;;
      cache-valid-record)
        cache_key="${endpoint#*key=}"
        cache_key="${cache_key%%&*}"
        jq -cn --arg key "$cache_key" \
          '{total_count:1,actions_caches:[{id:5,key:$key,ref:"refs/heads/main",size_in_bytes:1024,last_accessed_at:null}]}' ;;
      *) printf '%s\n' '{"total_count":0,"actions_caches":[]}' ;;
    esac ;;
  POST:/repos/tailrocks/velnor-new/actions/runs/123/cancel)
    test "$include" = true
    printf 'HTTP/2 202 Accepted\r\n\r\n' ;;
  *) printf 'unexpected fixture request: %s %s\n' "$method" "$endpoint" >&2; exit 91 ;;
esac
"#;

pub(super) const FAKE_REALPATH: &str = r#"#!/usr/bin/env bash
set -euo pipefail
[ "$#" = 3 ] && [ "$1" = -e ] && [ "$2" = -- ] || exit 2
path="$3"
[ -e "$path" ] || exit 1
if [ -d "$path" ]; then
  (cd -P -- "$path" && pwd -P)
else
  parent="$(cd -P -- "$(dirname -- "$path")" && pwd -P)"
  printf '%s/%s\n' "$parent" "$(basename -- "$path")"
fi
"#;

pub(super) const FAKE_STAT: &str = r#"#!/usr/bin/env bash
set -euo pipefail
if [ "$1" = -c ]; then format="$2"; path="$4"; else format="$2"; path="$3"; fi
case "$format" in
  %u|%h|%s|%a) ;;
  *) exit 2 ;;
esac
case "$format" in %h) format=%l ;; %s) format=%z ;; %a) format=%Lp ;; esac
exec /usr/bin/stat -f "$format" "$path"
"#;
