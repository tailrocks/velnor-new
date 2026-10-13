//! Fake transport that asserts fixed hosts, redirect boundaries, and byte caps.

pub(super) const FAKE_CURL: &str = r#"#!/usr/bin/env bash
set -euo pipefail
headers=
output=
config=
url=
max_filesize=
proto_redir=
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
    --write-out|--proto|--proto-redir|--connect-timeout|--max-time|--max-redirs) shift 2 ;;
    --location) follow=true; shift ;;
    --silent|--show-error|--fail) shift ;;
    https://*) url="$1"; shift ;;
    *) shift ;;
  esac
done
if [ -n "$config" ]; then url="$(sed -n 's/^url = "\(.*\)"$/\1/p' "$config")"; fi
if [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/artifacts/55/zip ]; then
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
  test "$follow" = true
  test "$authorized" = false
  test "$max_filesize" = 1048576
  test "$proto_redir" = =https
  printf 'observer-signed authorized=false\n' >> "$CURL_LOG"
  if [ "$CURL_LOCATION_MODE" = signed-http-redirect ]; then
    printf 'HTTP/2 302 Found\r\nLocation: http://signed.example/blocked\r\n\r\n' > "$headers"
  else
    printf 'HTTP/2 200 OK\r\n\r\n' > "$headers"
  fi
  if [ "$CURL_LOCATION_MODE" = large-body ]; then
    head -c 1048577 /dev/zero
  else
    case "$CURL_MARKER" in
    partial) printf '2026-10-04T00:00:05.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n' ;;
    rounded100) printf '2026-10-04T00:00:05.0000000Z Sent 9996 of 10000 (100.0%%), 0.1 MBs/sec\n' ;;
    short-timestamp) printf '2026-10-04T00:00:05.000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n' ;;
    wrong-separator) printf '2026/10/04T00:00:05.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n' ;;
    invalid-calendar) printf '2026-02-30T00:00:05.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n' ;;
    late) printf '2026-10-04T00:00:11.0000000Z Sent 256 of 1024 (25.0%%), 0.1 MBs/sec\n' ;;
    zero) printf '2026-10-04T00:00:05.0000000Z Sent 0 of 1024 (0.0%%), 0.1 MBs/sec\n' ;;
    complete) printf '2026-10-04T00:00:05.0000000Z Sent 1024 of 1024 (100.0%%), 0.1 MBs/sec\n' ;;
    malformed) printf '2026-10-04T00:00:05.0000000Z Sent many of 1024 (25.0%%), 0.1 MBs/sec\n' ;;
      *) printf 'ordinary save log without progress evidence\n' ;;
      esac
  fi
elif [ "$url" = https://signed.example/restore-log ]; then
  test "$disabled" = true
  test "$follow" = true
  test "$authorized" = false
  test "$max_filesize" = 1048576
  test "$proto_redir" = =https
  printf 'restore-signed authorized=false\n' >> "$CURL_LOG"
  if [ "$RESTORE_TRANSPORT_MODE" = signed-http-redirect ]; then
    printf 'HTTP/2 302 Found\r\nLocation: http://signed.example/blocked\r\n\r\n' > "$headers"
  elif [ "$RESTORE_TRANSPORT_MODE" = large-signed-header ]; then
    fill="$(head -c 70000 /dev/zero | tr '\000' A)"
    printf 'HTTP/2 200 OK\r\nX-Fill: %s\r\n\r\n' "$fill" > "$headers"
  else
    printf 'HTTP/2 200 OK\r\n\r\n' > "$headers"
  fi
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
elif [[ "$url" == https://api.github.com/*/steps/*/logs ]]; then
  exit 93
else
  exit 92
fi
"#;
