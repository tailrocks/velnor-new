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
  if [ "$label" = restore-step-end ] && [ "$MBX_QUALIFICATION_ROLE" != writer ]; then check_nonempty_root "$label" bundle; fi
  if [ "$label" = export-complete ] && { [ "$MBX_QUALIFICATION_ROLE" = writer ] || [ "$MBX_QUALIFICATION_ROLE" = seed ] || [ "$MBX_QUALIFICATION_ROLE" = new-key-writer ]; }; then
    check_nonempty_root "$label" selected-cache-root
    check_nonempty_root "$label" bundle
  fi
  if [ "$label" = import-step-end ] && [ "$MBX_QUALIFICATION_ROLE" = reader ]; then check_nonempty_root "$label" selected-cache-root; fi
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
  if [ -s "$evidence/sampler-stop.tsv" ] && ! awk -F '\t' '$1 == "remaining_group_members" { found=1; if ($2 != "0") bad=1 } END { exit !(found && !bad) }' "$evidence/sampler-stop.tsv"; then
    fail_partial 'sampler stop receipt does not prove empty process group'
  fi
  case "$MBX_QUALIFICATION_ROLE" in
    writer|seed|new-key-writer)
      if [ ! -s "$MBX_QUALIFICATION_EXPORT_RECEIPT" ]; then fail_incomplete 'export_receipt_missing'; fi
      ;;
    reader|reader-a|reader-b|corrupt-reader)
      if [ ! -s "$MBX_QUALIFICATION_IMPORT_RECEIPT" ]; then fail_incomplete 'import_receipt_missing'; fi
      ;;
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
    --arg generation "$MBX_QUALIFICATION_CACHE_GENERATION" --arg rustc "$MBX_QUALIFICATION_RUSTC_IDENTITY" \
    '.job_id == $job and .role == $role and .run_id == $run and .run_attempt == $attempt and .source_sha == $sha and .source_ref == $source_ref and .workflow_ref == $workflow_ref and .scope == $scope and .mbx_action_ref == $action and .mbx_version == $mbx and .rust_version == $rust and .primary_key == $primary and .cache_prefix == $prefix and .generation == $generation and .rustc_identity == $rustc and .primary_key != "" and .generation != "" and .rustc_identity != ""' "$receipt" >/dev/null 2>&1; then
    fail_incomplete 'cache receipt identity binding failed'
    return
  fi
  case "$MBX_QUALIFICATION_ROLE" in
    writer|seed|new-key-writer)
      jq -e '.cache_hit == "false" and .matched_key == "" and .export_ready == "true" and .export_status == "0" and .gc_status == "0" and .save_outcome == "success"' "$receipt" >/dev/null 2>&1 || fail_incomplete 'writer cache lifecycle receipt failed'
      ;;
    reader|reader-a|reader-b)
      jq -e '.cache_hit == "true" and .matched_key == .primary_key and .imported_objects > 0 and .cached_compilations > 0' "$receipt" >/dev/null 2>&1 || fail_incomplete 'reader cache reuse receipt failed'
      ;;
    corrupt-reader)
      jq -e '.cache_hit == "true" and .matched_key == .primary_key and .import_status != "" and .import_status != "0" and .selected_cache_root != "" and .abandoned_import_root != "" and .selected_cache_root != .abandoned_import_root and .imported_objects == 0 and .cached_compilations == 0' "$receipt" >/dev/null 2>&1 || fail_incomplete 'corrupt import fallback receipt failed'
      ;;
  esac
}
write_qualification_status() {
  local state=complete
  if [ "$incomplete" -ne 0 ]; then state=incomplete; elif [ "$partial" -ne 0 ]; then state=partial; fi
  printf 'qualification_status\t%s\n' "$state" > "$evidence/qualification-status.tsv"
  if [ "$state" != complete ]; then exit 1; fi
}
proc_identity() {
  local row remainder
  local -a fields=()
  IFS= read -r row < "/proc/$1/stat" || return 1
  remainder="${row##*) }"
  read -r -a fields <<< "$remainder"
  ((${#fields[@]} > 19)) || return 1
  printf '%s\t%s\t%s\n' "${fields[2]}" "${fields[3]}" "${fields[19]}"
}
group_member_count() {
  local rows pid pgid sid count=0
  rows="$(ps -eo pid=,pgid=,sid=)" || return 1
  while read -r pid pgid sid; do
    [[ "$pgid" == "$sampler_pgid" ]] || continue
    [[ "$sid" == "$sampler_sid" ]] || return 1
    count=$((count + 1))
  done <<< "$rows"
  printf '%s\n' "$count"
}
sampler_leader_matches() {
  local actual args expected
  actual="$(proc_identity "$sampler_pid" 2>/dev/null)" || return 1
  expected="$(printf '%s\t%s\t%s' "$sampler_pgid" "$sampler_sid" "$sampler_start_ticks")"
  [[ "$actual" == "$expected" ]] || return 1
  args="$(ps -p "$sampler_pid" -o args= 2>/dev/null)" || return 1
  case "$args" in *"$evidence/sampler.sh"*) return 0 ;; *) return 1 ;; esac
}
wait_for_sampler_group() {
  local limit="$1" count=0 members
  while (( count < limit )); do
    members="$(group_member_count)" || return 2
    [[ "$members" == 0 ]] && return 0
    sleep 1
    count=$((count + 1))
  done
  members="$(group_member_count)" || return 2
  [[ "$members" == 0 ]]
}
if ! bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" validate; then
  echo 'private evidence directory validation failed' >&2
  exit 1
fi
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
if [ -s "$evidence/sampler.pid" ] && [ -s "$evidence/sampler.session.tsv" ] && [ -s "$evidence/sampler.log" ]; then
  if [ "$(stat -c '%h:%a:%u:%g' -- "$evidence/sampler.session.tsv")" != "1:600:$(id -u):$(id -g)" ] || \
    [ "$(awk -F '\t' '$1 == "evidence_identity" { print $2 }' "$evidence/sampler.session.tsv")" != "$(cat -- "$evidence/private.identity")" ]; then
    fail_partial 'sampler session receipt is unsafe or unbound'
  else
    while IFS=$'\t' read -r key value; do
      case "$key" in
        pid) sampler_pid="$value" ;;
        pgid) sampler_pgid="$value" ;;
        sid) sampler_sid="$value" ;;
        start_ticks) sampler_start_ticks="$value" ;;
      esac
    done < "$evidence/sampler.session.tsv"
  fi
  if [[ "$sampler_pid" =~ ^[0-9]+$ && "$sampler_pgid" == "$sampler_pid" && \
    "$sampler_sid" == "$sampler_pid" && "$sampler_start_ticks" =~ ^[0-9]+$ ]]; then
    if [ "$(cat -- "$evidence/sampler.pid")" != "$sampler_pid" ]; then
      fail_partial 'sampler pid differs from private session receipt'
    fi
    if [ -e "$evidence/sampler.stop" ] || [ -L "$evidence/sampler.stop" ]; then
      fail_partial 'sampler stop marker already exists'
    elif ! (set -o noclobber; : > "$evidence/sampler.stop"); then
      fail_partial 'sampler stop marker could not be created exclusively'
    fi
    if ! wait_for_sampler_group "$MBX_QUALIFICATION_FINALIZER_WAIT"; then
      members="$(group_member_count 2>/dev/null)" || members=unknown
      if [[ "$members" =~ ^[0-9]+$ ]] && (( members > 0 )) && \
        { ! kill -0 "$sampler_pid" 2>/dev/null || sampler_leader_matches; }; then
        fail_partial 'sampler session exceeded stop deadline; sending TERM to its isolated group'
        kill -TERM -- "-$sampler_pgid" 2>/dev/null || fail_partial 'sampler group TERM failed'
        wait_for_sampler_group 5 || true
        members="$(group_member_count 2>/dev/null)" || members=unknown
        if [[ "$members" =~ ^[0-9]+$ ]] && (( members > 0 )); then
          fail_partial 'sampler group remained after TERM; sending KILL to its isolated group'
          kill -KILL -- "-$sampler_pgid" 2>/dev/null || fail_partial 'sampler group KILL failed'
          wait_for_sampler_group 10 || fail_partial 'sampler process group did not terminate'
        elif [[ "$members" == unknown ]]; then
          fail_partial 'sampler process group identity changed during shutdown'
        fi
      else
        fail_partial 'sampler process group identity could not be verified for shutdown'
      fi
    fi
    members="$(group_member_count 2>/dev/null)" || members=unknown
    if [[ "$members" != 0 ]]; then fail_partial 'sampler process group is not proven empty'; fi
    test ! -e "$evidence/sampler-stop.tsv"
    test ! -L "$evidence/sampler-stop.tsv"
    (
      set -o noclobber
      printf 'pid\t%s\npgid\t%s\nsid\t%s\nremaining_group_members\t%s\nexit_status\t%s\n' \
        "$sampler_pid" "$sampler_pgid" "$sampler_sid" "$members" \
        "$(cat -- "$evidence/sampler.exit" 2>/dev/null || echo missing)" > "$evidence/sampler-stop.tsv"
    ) || fail_partial 'sampler stop receipt creation failed'
  else
    fail_partial 'sampler session identity malformed'
  fi
  if [ ! -s "$evidence/sampler.exit" ] || [ "$(cat "$evidence/sampler.exit")" != 0 ]; then
    fail_partial 'sampler did not report a clean exit'
  fi
else
  fail_partial 'sampler pid, session receipt, or log missing'
fi
if ! bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" snapshot final; then
  fail_partial 'final inventory snapshot failed'
fi
printf 'sampler_stopped_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" >> "$evidence/sampler-state.txt"
for label in $MBX_QUALIFICATION_EXPECTED_INVENTORIES final; do
  check_inventory "$label"
done
check_required_files
check_samples
check_receipt
write_qualification_status
