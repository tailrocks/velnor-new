set -uo pipefail
umask 077
evidence="$RUNNER_TEMP/mbx-cache-evidence"
fail_partial() { printf '%s\n' "$1" >> "$evidence/qualification-errors.txt"; partial=1; }
fail_incomplete() { printf '%s\n' "$1" >> "$evidence/qualification-errors.txt"; incomplete=1; }
check_nonempty_root() {
  local label="$1" root="$2" file="$evidence/inventory-summary-$1.tsv"
  if ! awk -F '\t' -v root="$root" 'NR > 1 && $1 == root { seen=1; if ($3 == 0 || $7 != "false") bad=1 } END { exit !(seen && !bad) }' "$file"; then
    fail_partial "required_nonempty_root_missing:${label}:${root}"
  fi
}
check_inventory_counts() {
  local label="$1" roots="$evidence/roots-$1.tsv" inventory="$evidence/inventory-$1.tsv"
  local summary="$evidence/inventory-summary-$1.tsv"
  if ! awk -F '\t' '
    FILENAME == ARGV[1] {
      if (NF != 4 || $1 == "" || $3 !~ /^[0-9]+$/ || $4 !~ /^[0-9]+$/) bad=1
      roots[$1]++; next
    }
    FILENAME == ARGV[2] {
      if (FNR == 1) next
      if (NF != 8 || $1 == "" || $4 !~ /^[0-9]+$/ || $5 !~ /^[0-9]+$/ ||
          $7 !~ /^[0-9]+$/ || $8 !~ /^[0-9]+$/) bad=1
      key=$1 SUBSEP $4 SUBSEP $5
      if (!seen[key]++) allocated[$1]+=$8
      files[$1]++; logical[$1]+=$7; inventory_roots[$1]++; next
    }
    FILENAME == ARGV[3] {
      if (FNR == 1) next
      if (NF != 7 || $1 == "" || $3 !~ /^[0-9]+$/ || $4 !~ /^[0-9]+$/ ||
          $5 !~ /^[0-9]+$/ || $6 !~ /^[0-9]+$/ || ($7 != "true" && $7 != "false")) bad=1
      summaries[$1]++; expected_files[$1]=$3; expected_logical[$1]=$4
      expected_allocated[$1]=$5; next
    }
    END {
      for (root in roots) if (roots[root] != 1 || summaries[root] != 1) bad=1
      for (root in summaries) {
        if (!roots[root] || summaries[root] != 1 || files[root] + 0 != expected_files[root] + 0 ||
            logical[root] + 0 != expected_logical[root] + 0 ||
            allocated[root] + 0 != expected_allocated[root] + 0) bad=1
      }
      for (root in inventory_roots) if (!roots[root] || !summaries[root]) bad=1
      exit bad
    }
  ' "$roots" "$inventory" "$summary"; then
    fail_partial "inventory_count_or_total_mismatch:$label"
  fi
}
check_inventory() {
  local label="$1" file
  for file in "roots-$label.tsv" "inventory-$label.tsv" "inventory-summary-$label.tsv" "inventory-meta-$label.tsv" "root-status-$label.tsv" "df-$label-bytes.txt" "df-$label-inodes.txt"; do
    if [ ! -s "$evidence/$file" ]; then fail_partial "required_inventory_missing:$label:$file"; fi
  done
  if [ -s "$evidence/root-status-$label.tsv" ]; then
    if ! awk -F '\t' 'NR > 1 { rows++; if ($3 == "true" && $2 != "present") bad=1 } END { exit !(rows > 0 && !bad) }' "$evidence/root-status-$label.tsv"; then
      fail_partial "required_root_missing:$label"
    fi
  fi
  if [ -s "$evidence/roots-$label.tsv" ] && [ -s "$evidence/inventory-$label.tsv" ] && \
    [ -s "$evidence/inventory-summary-$label.tsv" ]; then
    check_inventory_counts "$label"
  fi
  if [ "$label" = export-complete ]; then check_hash_totals "$label"; fi
  if [ -s "$evidence/root-registry.tsv" ] && ! grep -Fq "$label"$'\t' "$evidence/root-registry.tsv"; then
    fail_partial "root_registry_missing:$label"
  fi
  if [ -s "$evidence/inventory-summary-$label.tsv" ] && awk -F '\t' 'NR > 1 && $7 == "true" { found=1 } END { exit !found }' "$evidence/inventory-summary-$label.tsv"; then
    fail_partial "inventory_truncated:$label"
  fi
  if [ -s "$evidence/df-$label-bytes.txt" ] && ! grep -qx 'df_status=ok' "$evidence/df-$label-bytes.txt"; then fail_partial "df_bytes_invalid:$label"; fi
  if [ -s "$evidence/df-$label-inodes.txt" ] && ! grep -qx 'df_status=ok' "$evidence/df-$label-inodes.txt"; then fail_partial "df_inodes_invalid:$label"; fi
  if [ "$label" = restore-step-end ]; then
    case "$(resource_role_class)" in
      hit) check_nonempty_root "$label" bundle ;;
      cold) ;;
      *) fail_partial "unknown_cache_role:${MBX_QUALIFICATION_ROLE-}" ;;
    esac
  fi
  if [ "$label" = export-complete ] && [ "$(resource_role_class)" = cold ]; then
    check_nonempty_root "$label" selected-cache-root
    check_nonempty_root "$label" bundle
  fi
  if [ "$label" = import-step-end ] && [ "$(resource_role_class)" = hit ] && [ "$MBX_QUALIFICATION_ROLE" != corrupt-reader ]; then check_nonempty_root "$label" selected-cache-root; fi
  if [ "$label" = build-end ] || [ "$label" = final ]; then
    check_nonempty_root "$label" MBX_TARGET_ROOT
    check_nonempty_root "$label" cargo-home
  fi
}
check_hash_totals() {
  local label="$1" hashes="$evidence/content-hashes-$1.tsv"
  local summary="$evidence/duplicate-content-summary-$1.tsv"
  if [ ! -s "$hashes" ] || [ ! -s "$summary" ] || ! awk -F '\t' '
    FILENAME == ARGV[1] { if (FNR > 1) { if ($3 !~ /^[0-9]+$/) bad=1; rows++; total+=$3 } next }
    $1 == "hashed_regular_file_count" { expected_rows=$2; rows_found=1 }
    $1 == "hashed_logical_bytes" { expected_bytes=$2; bytes_found=1 }
    END {
      if (bad || !rows_found || !bytes_found || expected_rows !~ /^[0-9]+$/ ||
          expected_bytes !~ /^[0-9]+$/ || rows == 0 || rows != expected_rows || total != expected_bytes) exit 1
    }
  ' "$hashes" "$summary"; then
    fail_partial "hash_inventory_mismatch:$label"
  fi
}
check_required_files() {
  local file
  for file in runner-metadata.json samples.jsonl sampling-summary.json inventory-final.tsv inventory-meta-final.tsv root-status-final.tsv cache-receipt.json phases.tsv sampler-stop.tsv mbx-cache-stats-import-step-end.json mbx-stats-build-end.json mbx-cache-stats-import-step-end.exit mbx-stats-build-end.exit; do
    if [ ! -s "$evidence/$file" ]; then fail_incomplete "required_file_missing:$file"; fi
  done
  if [ -s "$evidence/sampler-stop.tsv" ] && ! awk -F '\t' '$1 == "remaining_session_members" { found=1; if ($2 != "0") bad=1 } END { exit !(found && !bad) }' "$evidence/sampler-stop.tsv"; then
    fail_partial 'sampler stop receipt does not prove empty owned session'
  fi
  if [ -s "$evidence/sampler-stop.tsv" ] && ! awk -F '\t' '
    $1 == "shutdown_deadline_status" { status=$2; status_found++ }
    $1 == "shutdown_budget_seconds" { budget=$2; budget_found++ }
    $1 == "shutdown_elapsed_centiseconds" { elapsed=$2; elapsed_found++ }
    $1 == "shutdown_graceful_elapsed_centiseconds" { graceful=$2; graceful_found++ }
    $1 == "shutdown_term_elapsed_centiseconds" { term=$2; term_found++ }
    $1 == "shutdown_kill_elapsed_centiseconds" { killed=$2; kill_found++ }
    $1 == "process_scan_limit" { scan_limit=$2; scan_limit_found++ }
    END {
      if (status_found != 1 || status != "within_budget" || budget_found != 1 ||
          elapsed_found != 1 || graceful_found != 1 || term_found != 1 || kill_found != 1 ||
          scan_limit_found != 1 || scan_limit != "4096" || budget !~ /^[1-9][0-9]*$/ ||
          elapsed !~ /^[0-9]+$/ || graceful !~ /^[0-9]+$/ || term !~ /^[0-9]+$/ ||
          killed !~ /^[0-9]+$/ || elapsed > budget * 100 || graceful > budget * 100 ||
          term > budget * 100 || killed > budget * 100 ||
          graceful + term + killed > elapsed || term != 0 || killed != 0) exit 1
    }
  ' "$evidence/sampler-stop.tsv"; then
    fail_partial 'sampler stop did not satisfy its monotonic wall-clock budget'
  fi
  case "$(resource_role_class)" in
    cold)
      if [ ! -s "$MBX_QUALIFICATION_EXPORT_RECEIPT" ]; then fail_incomplete 'export_receipt_missing'; fi
      ;;
    hit)
      if [ ! -s "$MBX_QUALIFICATION_IMPORT_RECEIPT" ]; then fail_incomplete 'import_receipt_missing'; fi
      ;;
    *) fail_incomplete "unknown_cache_role:${MBX_QUALIFICATION_ROLE-}" ;;
  esac
  if [ "$MBX_QUALIFICATION_ROLE" = corrupt-reader ]; then
    for file in corrupt-bundle-before.tsv corrupt-bundle-after.tsv corrupt-bundle-before-summary.tsv \
      corrupt-bundle-after-summary.tsv corrupt-mutation.tsv mutated-target-before-stat.txt \
      mutated-target-after-stat.txt original-cache-root.txt fallback-cache-root.txt \
      corrupt-cache-stats-before-build.json corrupt-mbx-stats-before-build.json; do
      if [ ! -s "$evidence/$file" ]; then fail_incomplete "corrupt_evidence_missing:$file"; fi
    done
  fi
  for file in "$evidence/mbx-cache-stats-import-step-end.json" "$evidence/mbx-stats-build-end.json" "$evidence/cache-receipt.json"; do
    if [ -s "$file" ] && ! jq -e . "$file" >/dev/null; then fail_incomplete "invalid_json:$file"; fi
  done
  for file in "$evidence/mbx-cache-stats-import-step-end.exit" "$evidence/mbx-stats-build-end.exit"; do
    if [ -s "$file" ] && [ "$(cat "$file")" != 0 ]; then fail_incomplete "stats_command_failed:$file"; fi
  done
  if [ "$MBX_QUALIFICATION_ROLE" = corrupt-reader ]; then
    jq -e '.objects == 0 and .action_results == 0' "$evidence/corrupt-cache-stats-before-build.json" >/dev/null 2>&1 || fail_incomplete 'corrupt fallback store was not empty before build'
    jq -e '.savings.cached_compilations == 0' "$evidence/corrupt-mbx-stats-before-build.json" >/dev/null 2>&1 || fail_incomplete 'corrupt fallback compilation count was not cold'
    if [ -s "$evidence/fallback-cache-root.txt" ] && [ -s "$evidence/final-cache-root.txt" ]; then
      cmp -s "$evidence/fallback-cache-root.txt" "$evidence/final-cache-root.txt" || fail_incomplete 'selected fallback root changed after corrupt import'
    else
      fail_incomplete 'selected fallback root evidence missing'
    fi
  fi
}
check_samples() {
  if ! jq -e '.resource_samples_complete == true and .sample_count >= 2 and .maximum_sample_gap_seconds <= ((.sample_interval_seconds * 2) + 5)' "$evidence/sampling-summary.json" >/dev/null 2>&1; then
    fail_partial 'resource_sampling_incomplete'
  fi
  if ! jq -se 'length >= 2 and all(.[]; .df_bytes_status == 0 and .df_inodes_status == 0 and .filesystem_used_bytes > 0 and .filesystem_used_inodes > 0)' "$evidence/samples.jsonl" >/dev/null 2>&1; then
    fail_partial 'resource_samples_invalid_or_empty'
  fi
  if ! jq -se 'length >= 2 and ([range(1;length) as $i | .[$i].elapsed_seconds > .[$i - 1].elapsed_seconds] | all)' "$evidence/samples.jsonl" >/dev/null 2>&1; then
    fail_partial 'resource_sample_time_not_monotonic'
  fi
  if [ -s "$evidence/inventory-errors.txt" ] || [ -s "$evidence/snapshot-errors.txt" ]; then
    fail_partial 'inventory_or_snapshot_error'
  fi
}
check_receipt() {
  local receipt="$evidence/cache-receipt.json"
  if [ ! -s "$receipt" ] || ! jq -e --arg job "$MBX_QUALIFICATION_JOB_ID" --arg role "$MBX_QUALIFICATION_ROLE" \
    --arg run "$GITHUB_RUN_ID" --arg attempt "$GITHUB_RUN_ATTEMPT" --arg sha "$GITHUB_SHA" \
    --arg source_ref "$GITHUB_REF" --arg workflow_ref "$GITHUB_WORKFLOW_REF" \
    --arg scope "$MBX_CACHE_SCOPE" --arg action "$MBX_QUALIFICATION_ACTION_REF" \
    --arg mbx "$MBX_VERSION" --arg rust "$RUSTUP_TOOLCHAIN" \
    --arg primary "$MBX_QUALIFICATION_CACHE_PRIMARY" --arg prefix "$MBX_QUALIFICATION_CACHE_PREFIX" \
    --arg restore_primary "$MBX_QUALIFICATION_RESTORE_PRIMARY_KEY" \
    --arg restore_conclusion "$MBX_QUALIFICATION_RESTORE_CONCLUSION" \
    --arg generation "$MBX_QUALIFICATION_CACHE_GENERATION" --arg rustc "$MBX_QUALIFICATION_RUSTC_IDENTITY" \
    '.receipt_status == "provisional" and .job_id == $job and .role == $role and .run_id == $run and .run_attempt == $attempt and .source_sha == $sha and .source_ref == $source_ref and .workflow_ref == $workflow_ref and .scope == $scope and .mbx_action_ref == $action and .mbx_version == $mbx and .rust_version == $rust and .primary_key == $primary and .derived_primary_key == $primary and .restore_primary_key == $restore_primary and .restore_conclusion == $restore_conclusion and .cache_prefix == $prefix and .generation == $generation and .rustc_identity == $rustc and .primary_key != "" and .generation != "" and .rustc_identity != ""' "$receipt" >/dev/null 2>&1; then
    fail_incomplete 'cache receipt identity binding failed'
    return
  fi
  case "$(resource_role_class)" in
    cold)
      jq -e '.restore_miss_candidate == true and .restore_conclusion == "success" and .restore_primary_key == .primary_key and .cache_hit == "" and .matched_key == "" and .export_ready == "true" and .export_status == "0" and .gc_status == "0" and .save_outcome == "success"' "$receipt" >/dev/null 2>&1 || fail_incomplete 'writer cache lifecycle receipt failed'
      ;;
    hit)
      if [ "$MBX_QUALIFICATION_ROLE" = corrupt-reader ]; then
        jq -e '.restore_conclusion == "success" and .restore_primary_key == .primary_key and .cache_hit == "true" and .matched_key == .primary_key and .import_status != "" and .import_status != "0" and .selected_cache_root != "" and .abandoned_import_root != "" and .selected_cache_root != .abandoned_import_root and .imported_objects == 0 and .cached_compilations == 0' "$receipt" >/dev/null 2>&1 || fail_incomplete 'corrupt import fallback receipt failed'
        return
      fi
      jq -e '.restore_conclusion == "success" and .restore_primary_key == .primary_key and .cache_hit == "true" and .matched_key == .primary_key and .imported_objects > 0 and .cached_compilations > 0' "$receipt" >/dev/null 2>&1 || fail_incomplete 'reader cache reuse receipt failed'
      ;;
    *)
      fail_incomplete "unknown_cache_role:${MBX_QUALIFICATION_ROLE-}"
      ;;
  esac
}
write_qualification_status() {
  local state=complete
  if [ "$incomplete" -ne 0 ]; then state=incomplete; elif [ "$partial" -ne 0 ]; then state=partial; fi
  printf 'qualification_status\t%s\ncache_certification\tprovisional\n' "$state" > "$evidence/qualification-status.tsv"
  if [ "$state" != complete ]; then exit 1; fi
}
if ! bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" validate; then
  echo 'private evidence directory validation failed' >&2
  exit 1
fi
path_validation_sha='aeb6a305ae82c86a08c6b08d459747c551369be18b6b84c26fa7d0be5084589b'
printf '%s  %s\n' "$path_validation_sha" "$evidence/path-validation.sh" | sha256sum --check --status || exit 1
. "$evidence/path-validation.sh"
partial=0
incomplete=0
selected_root="${MBX_SELECTED_CACHE_ROOT:-${MBX_CACHE_DIR-}}"
runner_temp_real="$(realpath -e -- "$RUNNER_TEMP" 2>/dev/null || true)"
selected_root_real="$(realpath -e -- "$selected_root" 2>/dev/null || true)"
if [ -n "$selected_root" ] && [ -d "$selected_root" ] && [ ! -L "$selected_root" ] && \
  [ "$RUNNER_TEMP" = "$runner_temp_real" ] && [ "$selected_root" = "$selected_root_real" ]; then
  printf '%s\n' "$selected_root" > "$evidence/final-cache-root.txt"
else
  fail_partial 'selected cache root missing or unsafe at finalization'
fi
if command -v mbx >/dev/null 2>&1; then
  if mbx cache stats --json > "$evidence/final-cache-stats.json" 2> "$evidence/final-cache-stats.stderr"; then :; else
    stats_status=$?
    printf 'exit_status=%s\n' "$stats_status" >> "$evidence/final-cache-stats.stderr"
    fail_incomplete 'final MBX cache stats command failed'
  fi
  if mbx stats --json > "$evidence/final-mbx-stats.json" 2> "$evidence/final-mbx-stats.stderr"; then :; else
    stats_status=$?
    printf 'exit_status=%s\n' "$stats_status" >> "$evidence/final-mbx-stats.stderr"
    fail_incomplete 'final MBX stats command failed'
  fi
else
  fail_incomplete 'pinned MBX executable missing during finalization'
fi
sampler_pid=''
sampler_pgid=''
sampler_sid=''
sampler_start_ticks=''
sampler_uid=''
sampler_gid=''
sampler_stopped=0
session_verified=0
members=unknown
if [ -s "$evidence/sampler.pid" ] && [ -s "$evidence/sampler.session.tsv" ] && [ -s "$evidence/sampler.log" ]; then
  control_valid=1
  declare -A control=()
  if [ "$(stat -c '%h:%a:%u:%g' -- "$evidence/sampler.session.tsv")" != "1:600:$(id -u):$(id -g)" ] || \
    [ "$(cat -- "$evidence/private.marker")" != "$(evidence_marker)" ] || \
    [ "$(cat -- "$evidence/private.identity")" != "$(stat -c '%d:%i:%u:%g' -- "$evidence")" ]; then
    control_valid=0
  fi
  line_count=0
  while IFS= read -r line || [[ -n "$line" ]]; do
    line_count=$((line_count + 1))
    [[ "$line" == *$'\t'* ]] || { control_valid=0; break; }
    key="${line%%$'\t'*}"
    value="${line#*$'\t'}"
    [[ "$value" != *$'\t'* ]] || { control_valid=0; break; }
    case "$key" in pid|pgid|sid|start_ticks|run_id|run_attempt|job_id|uid|gid|evidence_identity) ;; *) control_valid=0; break ;; esac
    if [[ -z "$key" || -z "$value" || -n "${control[$key]+set}" ]]; then control_valid=0; break; fi
    control[$key]="$value"
  done < "$evidence/sampler.session.tsv"
  if [[ "$line_count" != 10 || "${#control[@]}" != 10 || "${control[run_id]-}" != "$GITHUB_RUN_ID" ||
    "${control[run_attempt]-}" != "$GITHUB_RUN_ATTEMPT" ||
    "${control[job_id]-}" != "$MBX_QUALIFICATION_JOB_ID" ||
    "${control[uid]-}" != "$(id -u)" || "${control[gid]-}" != "$(id -g)" ||
    "${control[evidence_identity]-}" != "$(cat -- "$evidence/private.identity")" ]]; then control_valid=0; fi
  sampler_pid="${control[pid]-}"
  sampler_pgid="${control[pgid]-}"
  sampler_sid="${control[sid]-}"
  sampler_start_ticks="${control[start_ticks]-}"
  sampler_uid="${control[uid]-}"
  sampler_gid="${control[gid]-}"
  if ! valid_session_leader_pid "$sampler_pid" || [[ "$sampler_pgid" != "$sampler_pid" ||
    "$sampler_sid" != "$sampler_pid" || ! "$sampler_start_ticks" =~ ^[1-9][0-9]*$ ]]; then control_valid=0; fi
  [[ "$(cat -- "$evidence/sampler.pid")" == "$sampler_pid" ]] || control_valid=0
  if (( control_valid == 1 )); then
    if resource_shutdown_owned_session "$MBX_QUALIFICATION_FINALIZER_WAIT" 5 10; then
      session_verified="$RESOURCE_SHUTDOWN_VERIFIED"
      members="$RESOURCE_SHUTDOWN_REMAINING"
      sampler_stopped=1
    else
      session_verified="$RESOURCE_SHUTDOWN_VERIFIED"
      members="$RESOURCE_SHUTDOWN_REMAINING"
      fail_partial "owned sampler shutdown failed: $RESOURCE_SHUTDOWN_STATUS"
    fi
  else
    fail_partial 'sampler session control identity is malformed or original leader is unverified'
  fi
  test ! -e "$evidence/sampler-stop.tsv"
  test ! -L "$evidence/sampler-stop.tsv"
  (
    set -o noclobber
    printf 'pid\t%s\npgid\t%s\nsid\t%s\nrun_id\t%s\nrun_attempt\t%s\njob_id\t%s\nuid\t%s\ngid\t%s\nremaining_session_members\t%s\nexit_status\t%s\nshutdown_budget_seconds\t%s\nshutdown_elapsed_centiseconds\t%s\nshutdown_graceful_elapsed_centiseconds\t%s\nshutdown_term_elapsed_centiseconds\t%s\nshutdown_kill_elapsed_centiseconds\t%s\nshutdown_deadline_status\t%s\nprocess_scan_limit\t%s\n' \
      "$sampler_pid" "$sampler_pgid" "$sampler_sid" "$GITHUB_RUN_ID" "$GITHUB_RUN_ATTEMPT" \
      "$MBX_QUALIFICATION_JOB_ID" "$(id -u)" "$(id -g)" "$members" \
      "$(cat -- "$evidence/sampler.exit" 2>/dev/null || echo missing)" \
      "${RESOURCE_SHUTDOWN_BUDGET_SECONDS:-0}" "${RESOURCE_SHUTDOWN_ELAPSED_CS:-unknown}" \
      "${RESOURCE_SHUTDOWN_GRACEFUL_ELAPSED_CS:-0}" "${RESOURCE_SHUTDOWN_TERM_ELAPSED_CS:-0}" \
      "${RESOURCE_SHUTDOWN_KILL_ELAPSED_CS:-0}" "${RESOURCE_SHUTDOWN_STATUS:-not_started}" \
      "${RESOURCE_SESSION_SCAN_LIMIT:-unknown}" > "$evidence/sampler-stop.tsv"
  ) || fail_partial 'sampler stop receipt creation failed'
  if [ ! -s "$evidence/sampler.exit" ] || [ "$(cat "$evidence/sampler.exit")" != 0 ]; then
    fail_partial 'sampler did not report a clean exit'
  fi
else
  fail_partial 'sampler pid, session receipt, or log missing'
fi
if (( sampler_stopped == 1 && session_verified == 1 )) &&
  [ "${RESOURCE_SHUTDOWN_STATUS:-}" = within_budget ]; then
  if ! bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" snapshot final; then
    fail_partial 'final inventory snapshot failed'
  fi
else
  fail_partial 'final inventory skipped because owned sampler session was not proven empty'
fi
printf 'sampler_stopped_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" >> "$evidence/sampler-state.txt"
for label in $MBX_QUALIFICATION_EXPECTED_INVENTORIES final; do
  check_inventory "$label"
done
check_required_files
check_samples
check_receipt
write_qualification_status
