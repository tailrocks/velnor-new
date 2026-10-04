#!/usr/bin/env bash
set -euo pipefail
headers=
output=
config=
url=
max_filesize=
proto_redir=
write_out=
authorized=false
disabled=false
follow=false
while (($#)); do
  case "$1" in
    --disable) disabled=true; shift ;;
    --config) config="$2"; shift 2 ;;
    --dump-header) headers="$2"; shift 2 ;;
    --output) output="$2"; shift 2 ;;
    --header) case "$2" in "Authorization: Bearer "*) authorized=true ;; esac; shift 2 ;;
    --max-filesize) max_filesize="$2"; shift 2 ;;
    --proto-redir) proto_redir="$2"; shift 2 ;;
    --write-out) write_out="$2"; shift 2 ;;
    --proto|--proto-redir|--connect-timeout|--max-time|--max-redirs) shift 2 ;;
    --location) follow=true; shift ;;
    --silent|--show-error|--fail) shift ;;
    https://*) url="$1"; shift ;;
    *) shift ;;
  esac
done
if [ -n "$config" ]; then url="$(sed -n 's/^url = "\(.*\)"$/\1/p' "$config")"; fi
write_http_status() {
  case "$write_out" in
    *STATUS:*) printf '\nSTATUS:%s\n' "$1" ;;
    *http_code*) printf '%s' "$1" ;;
  esac
}
if [[ "$url" == https://api.github.com/repos/tailrocks/velnor-new/* ]] && [ -z "$headers" ]; then
  test "$disabled" = true && test "$authorized" = true && test "$follow" = false
  test "$max_filesize" = 2097152 && test -n "$output"
  test "$write_out" = '%{http_code}'
fi
if [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/workflows/qualification.yml ]; then
  printf 'workflow-api authorized=%s\n' "$authorized" >> "$CURL_LOG"
  jq -cn '{id:77,path:".github/workflows/qualification.yml",state:"active"}' > "$output"
  write_http_status 200
  exit 0
fi
if [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/runs/1 ]; then
  printf 'api-fixture authorized=%s\n' "$authorized" >> "$CURL_LOG"
  case "${API_JSON_MODE:-valid}" in
    status-201) printf '%s\n' '{"id":1}' > "$output"; write_http_status 201 ;;
    duplicate) printf '%s\n' '{}' '{}' > "$output"; write_http_status 200 ;;
    oversized) head -c 2097153 /dev/zero > "$output"; write_http_status 200 ;;
    *) printf '%s\n' '{"id":1}' > "$output"; write_http_status 200 ;;
  esac
  exit 0
fi
if [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/runs/900 ]; then
  test "$authorized" = true && test -n "$output"
  printf 'restore-run-api authorized=true\n' >> "$CURL_LOG"
  controller_mode="$(jq -er '.inputs.mode' "$GITHUB_EVENT_PATH")"
  controller_probe="$(jq -er '.inputs.probe_id' "$GITHUB_EVENT_PATH")"
  controller_title="MBX cancellation $controller_mode"
  if [ -n "$controller_probe" ]; then controller_title="$controller_title $controller_probe"; fi
  jq -cn --arg sha "$GITHUB_SHA" --arg actor "$GITHUB_ACTOR" \
    --arg title "$controller_title" --argjson workflow "${OBSERVER_WORKFLOW_ID:-77}" \
    '{id:900,workflow_id:$workflow,path:".github/workflows/qualification.yml@main",
      repository:{full_name:"tailrocks/velnor-new"},head_repository:{full_name:"tailrocks/velnor-new"},
      event:"workflow_dispatch",head_branch:"main",head_sha:$sha,run_attempt:1,
      status:"in_progress",conclusion:null,display_title:$title,actor:{login:$actor}}' > "$output"
  write_http_status 200
elif [ "$url" = 'https://api.github.com/repos/tailrocks/velnor-new/actions/runs/900/attempts/1/jobs?per_page=100' ]; then
  test "$authorized" = true && test -n "$output"
  printf 'restore-jobs-api authorized=true\n' >> "$CURL_LOG"
  jq -cn --arg name "$OBSERVER_JOB_NAME" --arg sha "$GITHUB_SHA" \
    '{total_count:1,jobs:[{id:901,run_id:900,run_attempt:1,head_sha:$sha,
      name:$name,status:"in_progress",conclusion:null,steps:[
        {name:"Set up job",status:"completed",conclusion:"success",number:1},
        {name:"Prepare MBX bundle key",status:"completed",conclusion:"success",number:2},
        {name:"Restore MBX single bundle",status:"completed",conclusion:"success",number:13}]}]}' > "$output"
  write_http_status 200
elif [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123 ]; then
  test "$authorized" = true && test -n "$output"
  printf 'child-run-api authorized=true\n' >> "$CURL_LOG"
  run_count=0
  if [ -s "$GH_STATE.run-count" ]; then IFS= read -r run_count < "$GH_STATE.run-count"; fi
  run_count=$((run_count + 1))
  printf '%s\n' "$run_count" > "$GH_STATE.run-count"
  attempt=1
  if [ -n "${CHILD_SOURCE_SHA:-}" ]; then
    run_sha="$CHILD_SOURCE_SHA"
    actor="${CHILD_ACTOR:-github-actions[bot]}"
    status=completed
    conclusion='"cancelled"'
  else
    run_sha="$GITHUB_SHA"
    actor=github-actions[bot]
    status=in_progress
    conclusion=null
  fi
  if [ "$GH_MODE" = mismatch ]; then repo=attacker/repo; else repo="$GITHUB_REPOSITORY"; fi
  if [ "$GH_MODE" = initial-mismatch ] && [ "$run_count" = 1 ]; then
    actor=unexpected-actor
    run_sha=dddddddddddddddddddddddddddddddddddddddd
  fi
  if [ "$GH_MODE" = revalidate-actor ] && [ "$run_count" -ge 5 ]; then actor=unexpected-actor; fi
  if [ "$GH_MODE" = revalidate-attempt ] && [ "$run_count" -ge 5 ]; then attempt=2; fi
  case "$GH_MODE" in
    terminal-good|observer-*) status=completed; conclusion='"cancelled"' ;;
    terminal-invalid-status) status=$'bad\ninjected=true' ;;
    terminal-unknown-status) status=unexpected ;;
    terminal-invalid-conclusion)
      status=completed
      conclusion="$(jq -cn --arg value $'bad\ninjected=true' '$value')" ;;
    terminal-unknown-conclusion) status=completed; conclusion='"unexpected"' ;;
    terminal-null-conclusion) status=completed ;;
  esac
  jq -cn --arg repo "$repo" --arg sha "$run_sha" --arg mode "$VICTIM_MODE" \
    --arg probe "$PROBE_ID" --arg actor "$actor" --arg status "$status" \
    --argjson conclusion "$conclusion" --argjson attempt "$attempt" \
    '{id:123,workflow_id:77,path:".github/workflows/qualification.yml@main",
      repository:{full_name:$repo},head_repository:{full_name:"tailrocks/velnor-new"},
      event:"workflow_dispatch",head_branch:"main",head_sha:$sha,run_attempt:$attempt,
      status:$status,conclusion:$conclusion,
      display_title:("MBX cancellation " + $mode + " " + $probe),actor:{login:$actor}}' > "$output"
  write_http_status 200
elif [ "$url" = 'https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123/attempts/1/jobs?per_page=100' ]; then
  test "$authorized" = true && test -n "$output"
  printf 'child-jobs-api authorized=true\n' >> "$CURL_LOG"
  if [ -z "${CHILD_SOURCE_SHA:-}" ]; then
    if [ "$PROBE_PHASE" = pre-save ]; then
      jq -cn --arg name "$VICTIM_JOB_NAME" --arg sha "$GITHUB_SHA" \
        '{total_count:1,jobs:[{id:456,run_id:123,run_attempt:1,head_sha:$sha,name:$name,
          status:"in_progress",conclusion:null,steps:[
            {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
            {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
            {name:"Wait at MBX pre-save cancellation point",status:"in_progress",conclusion:null},
            {name:"Save MBX single bundle",status:"completed",conclusion:"skipped"}]}]}' > "$output"
    else
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
        save_conclusion='"cancelled"'
      fi
      jq -cn --arg name "$VICTIM_JOB_NAME" --arg sha "$GITHUB_SHA" \
        --arg job_status "$job_status" --arg save_status "$save_status" \
        --argjson save_conclusion "$save_conclusion" \
        '{total_count:1,jobs:[{id:456,run_id:123,run_attempt:1,head_sha:$sha,name:$name,
          status:$job_status,
          conclusion:(if $job_status == "completed" then "cancelled" else null end),steps:[
            {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
            {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
            {name:"Save MBX single bundle",status:$save_status,conclusion:$save_conclusion,number:13}]}]}' > "$output"
    fi
  elif [ "$GH_MODE" = observer-object-steps ]; then
    jq -cn --arg name "$VICTIM_JOB_NAME" --arg sha "$CHILD_SOURCE_SHA" \
      '{total_count:1,jobs:[{id:456,run_id:123,run_attempt:1,head_sha:$sha,name:$name,
        status:"completed",conclusion:"cancelled",steps:{save:{name:"Save MBX single bundle",
          status:"completed",conclusion:"cancelled"}}}]}' > "$output"
  elif [ "$GH_MODE" = observer-missing-save ]; then
    jq -cn --arg name "$VICTIM_JOB_NAME" --arg sha "$CHILD_SOURCE_SHA" \
      '{total_count:1,jobs:[{id:456,run_id:123,run_attempt:1,head_sha:$sha,name:$name,
        status:"completed",conclusion:"cancelled",steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"}]}]}' > "$output"
  elif [ "$GH_MODE" = observer-duplicate-save ]; then
    jq -cn --arg name "$VICTIM_JOB_NAME" --arg sha "$CHILD_SOURCE_SHA" \
      '{total_count:1,jobs:[{id:456,run_id:123,run_attempt:1,head_sha:$sha,name:$name,
        status:"completed",conclusion:"cancelled",steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
          {name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:13},
          {name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:14}]}]}' > "$output"
  else
    jq -cn --arg name "$VICTIM_JOB_NAME" --arg sha "$CHILD_SOURCE_SHA" \
      '{total_count:1,jobs:[{id:456,run_id:123,run_attempt:1,head_sha:$sha,name:$name,
        status:"completed",conclusion:"cancelled",steps:[
          {name:"Write MBX cancellation readiness receipt",status:"completed",conclusion:"success"},
          {name:"Upload MBX cancellation receipt",status:"completed",conclusion:"success"},
          {name:"Save MBX single bundle",status:"completed",conclusion:"cancelled",number:13,
            started_at:"2026-10-04T00:00:00Z"}]}]}' > "$output"
  fi
  write_http_status 200
elif [ "$url" = 'https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123/artifacts?per_page=100' ]; then
  test "$authorized" = true && test -n "$output"
  printf 'artifact-list-api authorized=true\n' >> "$CURL_LOG"
  artifacts_count=0
  if [ -s "$GH_STATE.artifacts-count" ]; then IFS= read -r artifacts_count < "$GH_STATE.artifacts-count"; fi
  artifacts_count=$((artifacts_count + 1))
  printf '%s\n' "$artifacts_count" > "$GH_STATE.artifacts-count"
  artifact_id=55
  if [ "$GH_MODE" = revalidate-artifact ] && [ "$artifacts_count" -ge 2 ]; then artifact_id=56; fi
  jq -cn --arg name "$VICTIM_ARTIFACT_NAME" --arg digest "$GH_ARTIFACT_DIGEST" \
    --argjson artifact_id "$artifact_id" --argjson size "$GH_ARTIFACT_SIZE" \
    '{total_count:1,artifacts:[{id:$artifact_id,name:$name,expired:false,size_in_bytes:$size,
      digest:("sha256:" + $digest),workflow_run:{id:123}}]}' > "$output"
  write_http_status 200
