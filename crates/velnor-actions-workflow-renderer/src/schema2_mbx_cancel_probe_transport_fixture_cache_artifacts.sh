elif [[ "$url" == 'https://api.github.com/repos/tailrocks/velnor-new/actions/caches?key='* ]]; then
  test "$authorized" = true && test -n "$output"
  printf 'cache-api authorized=true\n' >> "$CURL_LOG"
  cache_key="${url#*key=}"
  cache_key="${cache_key%%&*}"
  case "${GH_MODE:-good}" in
    cache-object) printf '%s\n' '{"total_count":0,"actions_caches":{}}' > "$output" ;;
    cache-null-response) printf '%s\n' 'null' > "$output" ;;
    cache-null-entry) printf '%s\n' '{"total_count":1,"actions_caches":[null]}' > "$output" ;;
    cache-missing-array) printf '%s\n' '{"count":0,"caches":[]}' > "$output" ;;
    cache-missing-total) printf '%s\n' '{"actions_caches":[]}' > "$output" ;;
    cache-count-mismatch) printf '%s\n' '{"total_count":1,"actions_caches":[]}' > "$output" ;;
    cache-truncated-page) printf '%s\n' '{"total_count":101,"actions_caches":[]}' > "$output" ;;
    cache-valid-record)
      jq -cn --arg key "$cache_key" \
        '{total_count:1,actions_caches:[{id:5,key:$key,ref:"refs/heads/main",size_in_bytes:1024,last_accessed_at:null}]}' > "$output" ;;
    *) printf '%s\n' '{"total_count":0,"actions_caches":[]}' > "$output" ;;
  esac
  write_http_status 200
elif [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/artifacts/55/zip ]; then
  test "$disabled" = true
  test "$authorized" = true
  test "$follow" = false
  test "$max_filesize" = 65536
  test "$headers" = -
  test "$output" = /dev/null
  test "$write_out" = '\nSTATUS:%{http_code}\n'
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
  write_http_status 302
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
  test "$authorized" = true
  test "$output" = /dev/null
  test "$write_out" = '%{http_code}'
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
  write_http_status 302
elif [ "$url" = https://api.github.com/repos/tailrocks/velnor-new/actions/jobs/901/steps/2/logs ]; then
  test "$disabled" = true
  test "$authorized" = true
  test "$follow" = false
  test "$max_filesize" = 65536
  test "$output" = /dev/null
  test "$write_out" = '%{http_code}'
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
  write_http_status 302
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
  if [ "$CURL_LOCATION_MODE" = signed-http-redirect ]; then
    write_http_status 302
  else
    write_http_status 200
  fi
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
  if [ "$RESTORE_TRANSPORT_MODE" = signed-http-redirect ]; then
    write_http_status 302
  else
    write_http_status 200
  fi
elif [[ "$url" == https://api.github.com/*/steps/*/logs ]]; then
  exit 93
else
  exit 92
fi
