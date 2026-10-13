#!/usr/bin/env bash
set -euo pipefail

evidence="$1"
runner_temp="$2"
interval="$4"
mode="${5:-sample}"
label="${6:-}"
max_files=20000
max_hash_bytes=$((1024 * 1024 * 1024))
walk_ancestor_paths=()
walk_ancestor_ids=()

[[ -d "$evidence" && ! -L "$evidence" && -f "$evidence/path-validation.sh" && ! -L "$evidence/path-validation.sh" ]] || exit 1
[[ "$(realpath -e -- "$evidence")" == "$evidence" && \
  "$(stat -c '%h:%a:%u:%g' -- "$evidence/path-validation.sh")" == "1:700:$(id -u):$(id -g)" ]] || exit 1
. "$evidence/path-validation.sh"

utc_now() { date -u +%Y-%m-%dT%H:%M:%S.%NZ; }

write_metadata() {
  jq -cn \
    --arg utc_started "$(utc_now)" --arg run_id "${GITHUB_RUN_ID-}" \
    --arg run_attempt "${GITHUB_RUN_ATTEMPT-}" --arg sha "${GITHUB_SHA-}" \
    --arg ref "${GITHUB_REF-}" --arg workflow_ref "${GITHUB_WORKFLOW_REF-}" \
    --arg runner_os "${RUNNER_OS-}" --arg runner_arch "${RUNNER_ARCH-}" \
    --arg image_os "${ImageOS-}" --arg image_version "${ImageVersion-}" \
    --arg cargo_home "${CARGO_HOME-}" --arg interval "$interval" \
    --arg max_runtime_seconds 2400 \
    '{utc_started:$utc_started,run_id:$run_id,run_attempt:$run_attempt,sha:$sha,ref:$ref,workflow_ref:$workflow_ref,runner_os:$runner_os,runner_arch:$runner_arch,image_os:$image_os,image_version:$image_version,cargo_home:$cargo_home,sample_interval_seconds:($interval|tonumber),max_runtime_seconds:($max_runtime_seconds|tonumber),scope:"filesystem containing runner.temp"}' \
    > "$evidence/runner-metadata.json"
  uname -a > "$evidence/uname.txt"
  capture_df start bytes
  capture_df start inodes
}

capture_df() {
  local label="$1" kind="$2" output status=0
  local file="$evidence/df-$label-$kind.txt"
  if [[ "$kind" == bytes ]]; then
    output="$(df -B1 -P "$runner_temp" 2>&1)" || status=$?
  else
    output="$(df -i -P "$runner_temp" 2>&1)" || status=$?
  fi
  if (( status == 0 )) && ! valid_df "$output" "$kind"; then status=2; fi
  printf '%s\n' "$output" > "$file"
  if (( status == 0 )); then printf 'df_status=ok\n' >> "$file"; else printf 'df_status=failed:%s\n' "$status" >> "$file"; fi
}

valid_df() {
  local output="$1" kind="$2"
  awk -v kind="$kind" '
    NR == 1 {
      if (kind == "bytes" && ($1 != "Filesystem" || $2 != "1-blocks" || $3 != "Used" || $4 != "Available")) bad=1
      if (kind == "inodes" && ($1 != "Filesystem" || $2 != "Inodes" || $3 != "IUsed" || $4 != "IFree")) bad=1
      next
    }
    NR == 2 {
      rows++
      if (NF < 6 || $2 !~ /^[0-9]+$/ || $3 !~ /^[0-9]+$/ || $4 !~ /^[0-9]+$/) bad=1
      next
    }
    { bad=1 }
    END { if (NR != 2 || rows != 1 || bad) exit 1 }
  ' <<< "$output"
}

sample_once() {
  local index="$1" start_epoch="$2" bytes_status=0 inodes_status=0
  local bytes inodes used_bytes used_inodes elapsed
  bytes="$(df -B1 -P "$runner_temp" 2>&1)" || bytes_status=$?
  inodes="$(df -i -P "$runner_temp" 2>&1)" || inodes_status=$?
  if (( bytes_status == 0 )) && ! valid_df "$bytes" bytes; then bytes_status=2; fi
  if (( inodes_status == 0 )) && ! valid_df "$inodes" inodes; then inodes_status=2; fi
  used_bytes="$(awk 'NR == 2 { print $3 }' <<< "$bytes")"
  used_inodes="$(awk 'NR == 2 { print $3 }' <<< "$inodes")"
  elapsed=$(( $(date +%s) - start_epoch ))
  jq -cn \
    --arg utc "$(utc_now)" --arg elapsed "$elapsed" \
    --arg bytes "$bytes" --arg bytes_status "$bytes_status" --arg used_bytes "$used_bytes" \
    --arg inodes "$inodes" --arg inodes_status "$inodes_status" --arg used_inodes "$used_inodes" \
    --arg device "$(awk 'NR == 2 { print $1 }' <<< "$bytes")" \
    --arg mount "$(awk 'NR == 2 { print $NF }' <<< "$bytes")" --arg index "$index" \
    '{index:($index|tonumber),utc:$utc,elapsed_seconds:($elapsed|tonumber),device:$device,mount:$mount,df_bytes:$bytes,df_bytes_status:($bytes_status|tonumber),filesystem_used_bytes:(try ($used_bytes|tonumber) catch null),df_inodes:$inodes,df_inodes_status:($inodes_status|tonumber),filesystem_used_inodes:(try ($used_inodes|tonumber) catch null)}' \
    >> "$evidence/samples.jsonl"
  if [[ "$used_bytes" =~ ^[0-9]+$ ]] && (( used_bytes > max_used_bytes )); then max_used_bytes=$used_bytes; fi
  if [[ "$used_inodes" =~ ^[0-9]+$ ]] && (( used_inodes > max_used_inodes )); then max_used_inodes=$used_inodes; fi
}

add_root() {
  local name="$1" path="$2" required="$3" real escaped identity dev ino reason=''
  if [[ -z "$path" || "$path" != /* ]]; then reason=missing-or-relative
  elif [[ "$path" == *$'\n'* || "$path" == *$'\t'* || "$path" == *$'\r'* ]]; then reason=control-character
  elif ! real="$(realpath -e -- "$path" 2>/dev/null)"; then reason=missing
  elif [[ "$path" != "$real" ]]; then reason=noncanonical-or-symlink
  elif [[ "$real" != "$canonical_runner_temp"/* ]]; then reason=outside-runner-temp
  elif [[ -L "$path" || ! -d "$path" ]]; then reason=not-directory
  fi
  escaped="$(escape_path "$path")"
  if [[ -n "$reason" ]]; then
    printf '%s\t%s\t%s\t%s\n' "$name" "$reason" "$required" "$escaped" >> "$root_status_file"
    if [[ "$required" == true || "$reason" == outside-runner-temp || "$reason" == noncanonical-or-symlink || "$reason" == control-character ]]; then
      printf 'root_%s_%s\n' "$name" "$reason" >> "$evidence/inventory-errors.txt"
      snapshot_status=1
    fi
    return 0
  fi
  identity="$(stat -c '%d %i' -- "$path")"
  read -r dev ino <<< "$identity"
  printf '%s\t%s\t%s\t%s\n' "$name" "$escaped" "$dev" "$ino" >> "$roots_file"
  printf '%s\t%s\t%s\t%s\t%s\n' "$snapshot_label" "$name" "$escaped" "$dev" "$ino" >> "$evidence/root-registry.tsv"
  printf '%s\tpresent\t%s\t%s\n' "$name" "$required" "$escaped" >> "$root_status_file"
}

inventory_tree() {
  local root_name="$1" root_encoded="$2" expected_dev="$3" expected_ino="$4"
  local root_path dev ino nlink size blocks file count=0
  local logical_sum=0 allocated_sum=0 truncated=false
  root_path="${root_encoded//%09/$'\t'}"
  root_path="${root_path//%0A/$'\n'}"
  root_path="${root_path//%0D/$'\r'}"
  root_path="${root_path//%25/%}"
  if [[ "$(realpath -e -- "$root_path" 2>/dev/null || true)" != "$root_path" || -L "$root_path" ]] || \
    ! capture_walk_ancestry "$root_path" || ! validate_walk_ancestry; then
    printf 'root_changed_before_walk\t%s\n' "$root_encoded" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
    return 0
  fi
  local before_identity
  before_identity="$(stat -c '%d %i' -- "$root_path")"
  if [[ "$before_identity" != "$expected_dev $expected_ino" ]]; then
    printf 'root_identity_changed_before_walk\t%s\n' "$root_encoded" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
    return 0
  fi
  local walk_file="$evidence/walk-$snapshot_label-$root_name.nul" walk_status=0
  [[ ! -e "$walk_file" && ! -L "$walk_file" ]] || {
    printf 'walk_output_already_exists\t%s\n' "$root_name" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
    return 0
  }
  if ! (set -o noclobber; : > "$walk_file"); then
    printf 'walk_output_create_failed\t%s\n' "$root_name" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
    return 0
  fi
  if find -P "$root_path" -type f -printf '%D\0%i\0%n\0%s\0%b\0%p\0' \
      2>> "$evidence/inventory-errors.txt" | head -z -n "$(((max_files + 1) * 6))" > "$walk_file"; then
    walk_status=0
  else
    walk_status=$?
  fi
  declare -A seen_inodes=()
  while IFS= read -r -d '' dev && IFS= read -r -d '' ino && IFS= read -r -d '' nlink && \
    IFS= read -r -d '' size && IFS= read -r -d '' blocks && IFS= read -r -d '' file; do
    if ! record_inventory_file "$root_name" "$root_encoded" "$dev" "$ino" "$nlink" "$size" "$blocks" "$file"; then
      snapshot_status=1
      break
    fi
  done < "$walk_file"
  unset seen_inodes
  if ! rm -- "$walk_file"; then
    printf 'walk_output_remove_failed\t%s\n' "$root_name" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
  fi
  if (( walk_status != 0 )); then
    printf 'find_walk_failed:%s\t%s\n' "$walk_status" "$root_name" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
  fi
  local after_identity
  after_identity="$(stat -c '%d %i' -- "$root_path")"
  if [[ "$after_identity" != "$before_identity" ]]; then
    printf 'root_identity_changed_during_walk\t%s\n' "$root_encoded" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
  fi
  if ! validate_walk_ancestry; then
    printf 'root_ancestor_identity_changed_during_walk\t%s\n' "$root_encoded" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
  fi
  if [[ "$truncated" == true ]]; then
    printf 'inventory_cap_reached\t%s\n' "$root_name" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
  fi
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$root_name" "$root_encoded" "$((count > max_files ? max_files : count))" "$logical_sum" "$allocated_sum" "$max_files" "$truncated" >> "$summary_file"
}

record_inventory_file() {
  local root_name="$1" root_encoded="$2" dev="$3" ino="$4" nlink="$5" size="$6" blocks="$7" file="$8"
  local allocated=$((blocks * 512)) inode="$dev:$ino" escaped
  count=$((count + 1))
  if (( count > max_files )); then truncated=true; return 1; fi
  logical_sum=$((logical_sum + size))
  if [[ -z "${seen_inodes[$inode]+x}" ]]; then
    seen_inodes[$inode]=1
    allocated_sum=$((allocated_sum + allocated))
  fi
  escaped="$(escape_path "$file")"
  if ! printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$root_name" "$root_encoded" "$escaped" "$dev" "$ino" "$nlink" "$size" "$allocated" >> "$inventory_file"; then
    snapshot_status=1
    return 1
  fi
  if [[ "$snapshot_label" == export-complete && ( "$root_name" == selected-cache-root || "$root_name" == bundle ) ]]; then
    hash_inventory_file "$root_name" "$dev" "$ino" "$nlink" "$size" "$blocks" "$file" "$escaped" || return 1
  fi
}

hash_inventory_file() {
  local root_name="$1" dev="$2" ino="$3" nlink="$4" size="$5" blocks="$6" file="$7" escaped="$8"
  local file_before file_after file_stat file_dev file_ino file_links file_size file_blocks file_mode hash hash_output
  if (( hash_bytes + size > max_hash_bytes )); then hash_cap_hit=true; return 0; fi
  file_stat="$(stat -c '%d %i %h %s %b %f' -- "$file")" || { snapshot_status=1; return 1; }
  read -r file_dev file_ino file_links file_size file_blocks file_mode <<< "$file_stat"
  if [[ -L "$file" || ! "$file_mode" =~ ^[[:xdigit:]]+$ ]] || \
    (( (16#$file_mode & 0xF000) != 0x8000 )) || [[ "$file_dev" != "$dev" ||
    "$file_ino" != "$ino" || "$file_links" != "$nlink" || "$file_size" != "$size" ||
    "$file_blocks" != "$blocks" || "$(realpath -e -- "$file" 2>/dev/null || true)" != "$file" ]]; then
    printf 'file_identity_changed_before_hash\t%s\n' "$escaped" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
    return 1
  fi
  file_before="${file_dev}:${file_ino}:${file_links}:${file_size}:${file_blocks}:${file_mode}"; if ! hash_output="$(sha256sum -- "$file")"; then
    printf 'sha256_failed\t%s\n' "$escaped" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
    return 1
  fi
  hash="${hash_output%% *}"
  if [[ ! "$hash" =~ ^[0-9a-f]{64}$ ]]; then
    printf 'sha256_invalid_output\t%s\n' "$escaped" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
    return 1
  fi
  file_after="$(stat -c '%d:%i:%h:%s:%b:%f' -- "$file")" || { snapshot_status=1; return 1; }
  [[ "$file_after" == "$file_before" && ! -L "$file" &&
    "$(realpath -e -- "$file" 2>/dev/null || true)" == "$file" ]] || {
    printf 'file_identity_changed_during_hash\t%s\n' "$escaped" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
    return 1
  }
  hash_bytes=$((hash_bytes + size)); hash_files=$((hash_files + 1))
  if ! printf '%s\t%s\t%s\t%s\n' "$root_name" "$hash" "$size" "$escaped" >> "$hashes_file"; then
    snapshot_status=1
    return 1
  fi
}

duplicate_content_summary() {
  local root_hashes="$evidence/root-hashes-$snapshot_label.tsv"
  local duplicate_file="$evidence/duplicate-content-$snapshot_label.tsv"
  local duplicate_count duplicate_bytes
  awk -F '\t' '$1 == "selected-cache-root" { print $2 }' "$hashes_file" | sort -u > "$root_hashes"
  awk -F '\t' 'FILENAME == ARGV[1] { roots[$1] = 1; next } $1 == "bundle" && roots[$2] { print $2 "\t" $3 "\t" $4 }' \
    "$root_hashes" "$hashes_file" > "$duplicate_file"
  duplicate_count="$(wc -l < "$duplicate_file" | tr -d ' ')"
  duplicate_bytes="$(awk -F '\t' '{ total += $2 } END { printf "%.0f", total }' "$duplicate_file")"
  {
    printf 'interpretation\tequal SHA-256 content only; does not prove shared or duplicated physical extents\n'
    printf 'root_bundle_matching_file_count\t%s\n' "$duplicate_count"
    printf 'root_bundle_matching_logical_bytes\t%s\n' "$duplicate_bytes"
    printf 'hashed_regular_file_count\t%s\n' "$hash_files"
    printf 'hashed_logical_bytes\t%s\n' "$hash_bytes"
    printf 'hash_byte_cap\t%s\n' "$max_hash_bytes"
    printf 'hash_cap_reached\t%s\n' "$hash_cap_hit"
  } > "$evidence/duplicate-content-summary-$snapshot_label.tsv"
  if [[ "$hash_cap_hit" == true ]]; then
    printf 'hash_byte_cap_reached\t%s\n' "$snapshot_label" >> "$evidence/inventory-errors.txt"
    snapshot_status=1
  fi
}

prepare_snapshot_files() {
  local snapshot_label="$1" file
  roots_file="$evidence/roots-$snapshot_label.tsv"
  inventory_file="$evidence/inventory-$snapshot_label.tsv"
  summary_file="$evidence/inventory-summary-$snapshot_label.tsv"
  hashes_file="$evidence/content-hashes-$snapshot_label.tsv"
  root_status_file="$evidence/root-status-$snapshot_label.tsv"
  for file in "$roots_file" "$inventory_file" "$summary_file" "$hashes_file" "$root_status_file"; do
    [[ ! -e "$file" && ! -L "$file" ]] || { echo 'snapshot output already exists' >&2; return 1; }
  done
  : > "$roots_file"
  : > "$hashes_file"
  printf 'name\tstatus\trequired\tpath_percent_escaped\n' > "$root_status_file"
}

register_snapshot_roots() {
  local snapshot_label="$1" role="$2" required=0
  snapshot_status=0
  hash_bytes=0; hash_files=0
  hash_cap_hit=false
  add_root cargo-home "${CARGO_HOME-}" true
  selected_root="${MBX_SELECTED_CACHE_ROOT:-${MBX_CACHE_DIR-}}"
  add_root selected-cache-root "$selected_root" true
  add_root MBX_TARGET_ROOT "${MBX_TARGET_ROOT-}" "$([[ "$snapshot_label" == final || "$snapshot_label" == build-end ]] && echo true || echo false)"
  add_root MBX_SHIMS_DIR "${MBX_SHIMS_DIR-}" false
  required=0
  [[ "$snapshot_label" == final || "$snapshot_label" == export-complete ]] && required=1
  if [[ "$snapshot_label" == restore-step-end && "$role" =~ ^(reader|reader-a|reader-b|corrupt-reader)$ ]]; then required=1; fi
  add_root bundle "$runner_temp/mbx-single-bundle" "$([[ "$required" == 1 ]] && echo true || echo false)"
  if [[ -s "$evidence/original-cache-root.txt" ]]; then
    IFS= read -r original_root < "$evidence/original-cache-root.txt"
    add_root abandoned-import-root "$original_root" true
  fi
  if [[ -s "$evidence/fallback-cache-root.txt" ]]; then
    IFS= read -r fallback_root < "$evidence/fallback-cache-root.txt"
    add_root selected-fallback-root "$fallback_root" true
  elif [[ "$role" == corrupt-reader && ( "$snapshot_label" == corrupt-import-verified || "$snapshot_label" == final ) ]]; then
    add_root selected-fallback-root '' true
  fi
  sort -u "$roots_file" -o "$roots_file"
}

write_inventory_headers() {
  local snapshot_label="$1"
  printf 'root_name\troot_path_percent_escaped\tfile_path_percent_escaped\tdevice\tinode\tlink_count\tlogical_bytes\tallocated_bytes_st_blocks_times_512\n' > "$inventory_file"
  printf 'root_name\troot_path_percent_escaped\tregular_files_observed\tlogical_bytes\tinode_allocated_bytes_sum_dedup_within_tree\tfile_cap\ttruncated\n' > "$summary_file"
  printf 'root_name\tsha256\tlogical_bytes\tfile_path_percent_escaped\n' > "$hashes_file"
}

inventory_snapshot() {
  snapshot_label="$1"
  role="${MBX_QUALIFICATION_ROLE-}"
  [[ "$snapshot_label" =~ ^[a-z0-9-]+$ ]] || { echo 'invalid snapshot label' >&2; return 2; }
  validate_evidence || { echo 'private evidence directory validation failed' >&2; return 1; }
  canonical_runner_temp="$(realpath -e -- "$runner_temp")" || return 1
  if [[ "$canonical_runner_temp" != "$runner_temp" ]]; then
    printf 'runner_temp_noncanonical\n' >> "$evidence/inventory-errors.txt"
    return 1
  fi
  prepare_snapshot_files "$snapshot_label" || return 1
  register_snapshot_roots "$snapshot_label" "$role"
  write_inventory_headers "$snapshot_label"
  while IFS=$'\t' read -r root_name root_encoded root_dev root_ino; do
    [[ -n "$root_name" ]] || continue
    inventory_tree "$root_name" "$root_encoded" "$root_dev" "$root_ino"
  done < "$roots_file"
  if [[ "$snapshot_label" == export-complete ]]; then duplicate_content_summary; fi
  {
    printf 'label\t%s\n' "$snapshot_label"
    printf 'utc\t%s\n' "$(utc_now)"
    printf 'environment_source\texplicit current MBX env and selected_cache_root output; GITHUB_ENV file not read\n'
    printf 'inventory_file_cap_per_tree\t%s\n' "$max_files"
    printf 'allocated_interpretation\tsum st_blocks*512 once per dev/inode within each tree; reflink/COW extent sharing is unknown\n'
    printf 'filesystem_actual_pressure_source\tdf cadence rows in samples.jsonl and boundary df files; filesystem may have other users\n'
  } > "$evidence/inventory-meta-$snapshot_label.tsv"
  capture_df "$snapshot_label" bytes
  capture_df "$snapshot_label" inodes
  if grep -q 'df_status=failed:' "$evidence/df-$snapshot_label-bytes.txt" "$evidence/df-$snapshot_label-inodes.txt"; then snapshot_status=1; fi
  return "$snapshot_status"
}

[[ "$interval" =~ ^[1-9][0-9]*$ ]] && (( interval <= 30 )) || { echo 'sample interval must be 1..30 seconds' >&2; exit 2; }
canonical_runner_temp="$(realpath -e -- "$runner_temp")" || exit 1
[[ "$canonical_runner_temp" == "$runner_temp" ]] || { echo 'runner.temp is not canonical' >&2; exit 1; }
if [[ "$mode" == snapshot ]]; then inventory_snapshot "$label"; exit $?; fi
if [[ "$mode" == validate ]]; then validate_evidence; exit $?; fi
[[ "$mode" == sample ]] || { echo "unsupported sampler mode: $mode" >&2; exit 2; }
validate_evidence || { echo 'private evidence directory validation failed' >&2; exit 1; }
sampler_exit() {
  local status=$?
  set +e
  trap - EXIT
  if validate_evidence && [[ ! -e "$evidence/sampler.exit.tmp" && ! -L "$evidence/sampler.exit.tmp" &&
    ! -e "$evidence/sampler.exit" && ! -L "$evidence/sampler.exit" ]]; then
    if (set -o noclobber; printf '%s\n' "$status" > "$evidence/sampler.exit.tmp"); then
      mv -T -- "$evidence/sampler.exit.tmp" "$evidence/sampler.exit"
    fi
  fi
  exit "$status"
}
trap sampler_exit EXIT
test ! -e "$evidence/sampler.exit"
test ! -e "$evidence/sampler.pid"
test ! -L "$evidence/sampler.pid"
(
  set -o noclobber
  printf '%s\n' "$$" > "$evidence/sampler.pid"
)
write_metadata
start_epoch="$(date +%s)"
max_used_bytes=0
max_used_inodes=0
sample_count=0
max_runtime_hit=false
while [[ ! -e "$evidence/sampler.stop" ]]; do
  validate_evidence || { echo 'private evidence directory identity changed during sampling' >&2; exit 1; }
  sample_once "$sample_count" "$start_epoch"
  sample_count=$((sample_count + 1))
  if (( $(date +%s) - start_epoch >= 2400 )); then max_runtime_hit=true; break; fi
  sleep "$interval"
done
validate_evidence || { echo 'private evidence directory identity changed before final sample' >&2; exit 1; }
sample_once "$sample_count" "$start_epoch"
sample_count=$((sample_count + 1))
jq -s \
  --arg interval "$interval" --arg max_bytes "$max_used_bytes" --arg max_inodes "$max_used_inodes" \
  --arg started "$(jq -r '.utc_started' "$evidence/runner-metadata.json")" \
  --arg ended "$(utc_now)" --arg max_runtime_hit "$max_runtime_hit" \
  '([range(1; length) as $i | .[$i].elapsed_seconds - .[$i-1].elapsed_seconds] | max // 0) as $gap |
   {sample_interval_seconds:($interval|tonumber),observed_max_filesystem_used_bytes_lower_bound:($max_bytes|tonumber),observed_max_filesystem_used_inodes_lower_bound:($max_inodes|tonumber),sample_count:length,maximum_sample_gap_seconds:$gap,observed_start_utc:$started,observed_end_utc:$ended,peak_is_instantaneous:false,used_values_include_other_filesystem_users:true,max_runtime_hit:($max_runtime_hit=="true"),resource_samples_complete:(length>=2 and $gap<=((($interval|tonumber)*2)+5) and all(.[]; .df_bytes_status==0 and .df_inodes_status==0 and .filesystem_used_bytes>0 and .filesystem_used_inodes>0) and ($max_bytes|tonumber)>0 and ($max_inodes|tonumber)>0 and $max_runtime_hit!="true"),max_runtime_seconds:2400}' \
  "$evidence/samples.jsonl" > "$evidence/sampling-summary.json"
