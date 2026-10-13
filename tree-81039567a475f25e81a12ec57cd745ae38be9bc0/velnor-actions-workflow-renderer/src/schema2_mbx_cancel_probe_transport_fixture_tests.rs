//! Fake transport that asserts fixed hosts, redirect boundaries, and byte caps.

pub(super) const FAKE_CURL: &str = r#"#!/usr/bin/env bash
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
if [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/runs/900 ]; then
  test "$authorized" = true && test -n "$output"
  printf 'restore-run-api authorized=true\n' >> "$CURL_LOG"
  controller_mode="$(jq -er '.inputs.mode' "$GITHUB_EVENT_PATH")"
  controller_probe="$(jq -er '.inputs.probe_id' "$GITHUB_EVENT_PATH")"
  controller_title="MBX cancellation $controller_mode"
  if [ -n "$controller_probe" ]; then controller_title="$controller_title $controller_probe"; fi
  jq -cn --arg sha "$GITHUB_SHA" --arg actor "$GITHUB_ACTOR" \
    --arg title "$controller_title" --argjson workflow "${OBSERVER_WORKFLOW_ID:-77}" \
    '{id:900,workflow_id:$workflow,path:".github/workflows/qualification.yml@refs/heads/main",
      repository:{full_name:"tailrocks/velnor-new"},head_repository:{full_name:"tailrocks/velnor-new"},
      event:"workflow_dispatch",head_branch:"main",head_sha:$sha,run_attempt:1,
      status:"in_progress",display_title:$title,actor:{login:$actor}}' > "$output"
  printf 200
elif [ "$url" = 'https://api.github.com/repos/tailrocks/velnor-new/actions/runs/900/attempts/1/jobs?per_page=100' ]; then
  test "$authorized" = true && test -n "$output"
  printf 'restore-jobs-api authorized=true\n' >> "$CURL_LOG"
  jq -cn --arg name "$OBSERVER_JOB_NAME" --arg sha "$GITHUB_SHA" \
    '{total_count:1,jobs:[{id:901,run_id:900,run_attempt:1,head_sha:$sha,
      name:$name,status:"in_progress",conclusion:null,steps:[
        {name:"Set up job",status:"completed",conclusion:"success",number:1},
        {name:"Prepare MBX bundle key",status:"completed",conclusion:"success",number:2},
        {name:"Restore MBX single bundle",status:"completed",conclusion:"success",number:13}]}]}' > "$output"
  printf 200
elif [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123 ]; then
  test "$authorized" = true && test -n "$output"
  printf 'child-run-api authorized=true\n' >> "$CURL_LOG"
  jq -cn --arg repo "$GITHUB_REPOSITORY" --arg sha "$CHILD_SOURCE_SHA" \
    --arg mode "$VICTIM_MODE" --arg probe "$PROBE_ID" --arg actor "$CHILD_ACTOR" \
    '{id:123,workflow_id:77,path:".github/workflows/qualification.yml@refs/heads/main",
      repository:{full_name:$repo},head_repository:{full_name:$repo},event:"workflow_dispatch",
      head_branch:"main",head_sha:$sha,run_attempt:1,status:"completed",conclusion:"cancelled",
      display_title:("MBX cancellation " + $mode + " " + $probe),actor:{login:$actor}}' > "$output"
  printf 200
elif [ "$url" = 'https://api.github.com/repos/tailrocks/velnor-new/actions/runs/123/attempts/1/jobs?per_page=100' ]; then
  test "$authorized" = true && test -n "$output"
  printf 'child-jobs-api authorized=true\n' >> "$CURL_LOG"
  if [ "$GH_MODE" = observer-object-steps ]; then
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
  printf 200
elif [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/artifacts/55/zip ]; then
  test "$disabled" = true
  test "$authorized" = true
  test "$follow" = false
  test "$max_filesize" = 65536
  test "$headers" = -
  test "$output" = /dev/null
  printf 'artifact-api authorized=true\n' >> "$CURL_LOG"
  case "$GH_MODE" in
    artifact-no-location) printf 'HTTP/2 302 Found\r\n\r\n' ;;
    artifact-http-location) printf 'HTTP/2 302 Found\r\nLocation: http://signed.example/archive\r\n\r\n' ;;
    artifact-duplicate-location)
      printf 'HTTP/2 302 Found\r\nLocation: https://signed.example/archive\r\nLocation: https://signed.example/other\r\n\r\n' ;;
    artifact-large-header)
      printf 'HTTP/2 302 Found\r\nLocation: https://signed.example/archive\r\nX-Fill: '
      head -c 70000 /dev/zero | tr '\000' A
      printf '\r\n\r\n'
      ;;
    *) printf 'HTTP/2 302 Found\r\nLocation: https://signed.example/archive?sig=fixture-only\r\n\r\n' ;;
  esac
elif [[ "$url" == https://signed.example/archive* ]]; then
  test "$disabled" = true
  test "$authorized" = false
  test "$follow" = false
  test "$max_filesize" = 1048576
  test "$output" = -
  printf 'artifact-signed authorized=false\n' >> "$CURL_LOG"
  if [ "$GH_MODE" = artifact-signed-redirect ] || [ "$GH_MODE" = artifact-signed-http-redirect ]; then
    printf 'HTTP/2 302 Found\r\nLocation: https://signed.example/other\r\n\r\n' > "$headers"
    if [ "$GH_MODE" = artifact-signed-http-redirect ]; then
      printf 'HTTP/2 302 Found\r\nLocation: http://signed.example/other\r\n\r\n' > "$headers"
    fi
  elif [ "$GH_MODE" = artifact-large-signed-header ]; then
    fill="$(head -c 70000 /dev/zero | tr '\000' A)"
    printf 'HTTP/2 200 OK\r\nX-Fill: %s\r\n\r\n' "$fill" > "$headers"
    cat "$GH_ARTIFACT_ZIP"
  else
    printf 'HTTP/2 200 OK\r\nContent-Type: application/zip\r\n\r\n' > "$headers"
    if [ "$GH_MODE" = artifact-large-body ]; then
      head -c 1048577 /dev/zero
    else
      cat "$GH_ARTIFACT_ZIP"
    fi
  fi
elif [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/jobs/456/steps/2/logs ]; then
  test "$disabled" = true
  test "$follow" = false
  test "$max_filesize" = 65536
  printf 'observer-api %s %s\n' "$url" "$authorized" >> "$CURL_LOG"
  case "$CURL_LOCATION_MODE" in
    missing) printf 'HTTP/2 302 Found\r\n\r\n' > "$headers" ;;
    malformed) printf 'HTTP/2 302 Found\r\nLocation: http://signed.example/log\r\n\r\n' > "$headers" ;;
    large-header)
      fill="$(head -c 70000 /dev/zero | tr '\000' A)"
      printf 'HTTP/2 302 Found\r\nLocation: https://signed.example/log\r\nX-Fill: %s\r\n\r\n' "$fill" > "$headers"
      ;;
    *) printf 'HTTP/2 302 Found\r\nLocation: https://signed.example/log\r\n\r\n' > "$headers" ;;
  esac
  printf 302
elif [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/jobs/901/steps/2/logs ]; then
  test "$disabled" = true
  test "$authorized" = true
  test "$follow" = false
  test "$max_filesize" = 65536
  test "$output" = /dev/null
  printf 'restore-api authorized=true\n' >> "$CURL_LOG"
  case "$RESTORE_TRANSPORT_MODE" in
    missing) printf 'HTTP/2 302 Found\r\n\r\n' > "$headers" ;;
    http-location) printf 'HTTP/2 302 Found\r\nLocation: http://signed.example/restore-log\r\n\r\n' > "$headers" ;;
    large-header)
      fill="$(head -c 70000 /dev/zero | tr '\000' A)"
      printf 'HTTP/2 302 Found\r\nLocation: https://signed.example/restore-log\r\nX-Fill: %s\r\n\r\n' "$fill" > "$headers"
      ;;
    *) printf 'HTTP/2 302 Found\r\nLocation: https://signed.example/restore-log\r\n\r\n' > "$headers" ;;
  esac
  printf 302
elif [ "$url" = https://signed.example/log ]; then
  test "$disabled" = true
  test "$follow" = false
  test "$authorized" = false
  test "$max_filesize" = 1048576
  printf 'observer-signed authorized=false\n' >> "$CURL_LOG"
  if [ "$CURL_LOCATION_MODE" = signed-http-redirect ]; then
    printf 'HTTP/2 302 Found\r\nLocation: http://signed.example/blocked\r\n\r\n' > "$headers"
  else
    printf 'HTTP/2 200 OK\r\n\r\n' > "$headers"
  fi
  {
  if [ "$CURL_LOCATION_MODE" = large-body ]; then
    head -c 1048577 /dev/zero
  else
    case "$CURL_MARKER" in
    partial)
      printf '2026-10-04T00:00:05.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n'
      printf '2026-10-04T00:00:06.0000000Z ##[error]The operation was canceled.\n' ;;
    rounded100)
      printf '2026-10-04T00:00:05.0000000Z Sent 9996 of 10000 (100.0%%), 0.1 MBs/sec\n'
      printf '2026-10-04T00:00:06.0000000Z ##[error]The operation was canceled.\n' ;;
    clock-skew)
      printf '2099-12-31T23:59:59.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n'
      printf '1999-01-01T00:00:00.0000000Z ##[error]The operation was canceled.\n' ;;
    reverse)
      printf '2099-12-31T23:59:59.0000000Z ##[error]The operation was canceled.\n'
      printf '1999-01-01T00:00:00.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n' ;;
    partial-no-error)
      printf '2026-10-04T00:00:05.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n' ;;
    malformed-error)
      printf '2026-10-04T00:00:05.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n'
      printf '2026-10-04T00:00:06.0000000Z ##[error]The operation was canceled\n'
      printf '2026-10-04T00:00:07.000Z ##[error]The operation was canceled.\n' ;;
    short-timestamp)
      printf '2026-10-04T00:00:05.000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n'
      printf '2026-10-04T00:00:06.0000000Z ##[error]The operation was canceled.\n' ;;
    wrong-separator)
      printf '2026/10/04T00:00:05.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n'
      printf '2026-10-04T00:00:06.0000000Z ##[error]The operation was canceled.\n' ;;
    invalid-calendar)
      printf '2026-02-30T00:00:05.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n'
      printf '2026-10-04T00:00:06.0000000Z ##[error]The operation was canceled.\n' ;;
    zero)
      printf '2026-10-04T00:00:05.0000000Z Sent 0 of 1024 (0.0%%), 0.1 MBs/sec\n'
      printf '2026-10-04T00:00:06.0000000Z ##[error]The operation was canceled.\n' ;;
    complete)
      printf '2026-10-04T00:00:05.0000000Z Sent 1024 of 1024 (100.0%%), 0.1 MBs/sec\n'
      printf '2026-10-04T00:00:06.0000000Z ##[error]The operation was canceled.\n' ;;
    malformed)
      printf '2026-10-04T00:00:05.0000000Z Sent many of 1024 (25.0%%), 0.1 MBs/sec\n'
      printf '2026-10-04T00:00:06.0000000Z ##[error]The operation was canceled.\n' ;;
    *) printf 'ordinary save log without progress evidence\n' ;;
      esac
  fi
  } > "$output"
  if [ "$CURL_LOCATION_MODE" = signed-http-redirect ]; then printf 302; else printf 200; fi
elif [ "$url" = https://signed.example/restore-log ]; then
  test "$disabled" = true
  test "$follow" = false
  test "$authorized" = false
  test "$max_filesize" = 1048576
  printf 'restore-signed authorized=false\n' >> "$CURL_LOG"
  if [ "$RESTORE_TRANSPORT_MODE" = signed-http-redirect ]; then
    printf 'HTTP/2 302 Found\r\nLocation: http://signed.example/blocked\r\n\r\n' > "$headers"
  elif [ "$RESTORE_TRANSPORT_MODE" = large-signed-header ]; then
    fill="$(head -c 70000 /dev/zero | tr '\000' A)"
    printf 'HTTP/2 200 OK\r\nX-Fill: %s\r\n\r\n' "$fill" > "$headers"
  else
    printf 'HTTP/2 200 OK\r\n\r\n' > "$headers"
  fi
  {
  if [ "$RESTORE_TRANSPORT_MODE" = large-body ]; then
    head -c 1048577 /dev/zero
  else
    case "$RESTORE_LOG_MODE" in
      clean) printf '2026-10-04T00:00:05.0000000Z Cache not found for input keys: %s\n' "$DERIVED_KEY" ;;
      failed) printf '2026-10-04T00:00:05.0000000Z ##[warning]Failed to restore: service unavailable\n2026-10-04T00:00:05.0000000Z Cache not found for input keys: %s\n' "$DERIVED_KEY" ;;
      short-timestamp) printf '2026-10-04T00:00:05.000Z Cache not found for input keys: %s\n' "$DERIVED_KEY" ;;
      no-marker) printf 'ordinary restore log\n' ;;
      *) printf 'Cache not found for input keys: %s\n' "$DERIVED_KEY" ;;
    esac
  fi
  } > "$output"
  if [ "$RESTORE_TRANSPORT_MODE" = signed-http-redirect ]; then printf 302; else printf 200; fi
elif [[ "$url" == https://api.github.com/*/steps/*/logs ]]; then
  exit 93
else
  exit 92
fi
"#;
