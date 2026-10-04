#!/usr/bin/env bash
set -euo pipefail

evidence="$1"
runner_temp="$2"
env_file="$3"
interval="$4"
mode="${5:-sample}"
label="${6:-}"
max_files=20000
max_hash_bytes=$((4 * 1024 * 1024 * 1024))

utc_now() {
  date -u +%Y-%m-%dT%H:%M:%S.%NZ
}

escape_path() {
  local value="$1"
  value="${value//%/%25}"
  value="${value//$'\t'/%09}"
  value="${value//$'\n'/%0A}"
  value="${value//$'\r'/%0D}"
  printf '%s' "$value"
}

unescape_path() {
  local value="$1"
  value="${value//%0D/$'\r'}"
  value="${value//%0A/$'\n'}"
  value="${value//%09/$'\t'}"
  value="${value//%25/%}"
  printf '%s' "$value"
}

write_metadata() {
  jq -cn \
    --arg utc_started "$(utc_now)" \
    --arg run_id "${GITHUB_RUN_ID-}" \
    --arg run_attempt "${GITHUB_RUN_ATTEMPT-}" \
    --arg sha "${GITHUB_SHA-}" \
    --arg ref "${GITHUB_REF-}" \
    --arg workflow_ref "${GITHUB_WORKFLOW_REF-}" \
    --arg runner_os "${RUNNER_OS-}" \
    --arg runner_arch "${RUNNER_ARCH-}" \
    --arg image_os "${ImageOS-}" \
    --arg image_version "${ImageVersion-}" \
    --arg cargo_home "${CARGO_HOME-}" \
    --arg interval "$interval" \
    --arg max_runtime_seconds 2700 \
    '{utc_started:$utc_started,run_id:$run_id,run_attempt:$run_attempt,sha:$sha,ref:$ref,workflow_ref:$workflow_ref,runner_os:$runner_os,runner_arch:$runner_arch,image_os:$image_os,image_version:$image_version,cargo_home:$cargo_home,sample_interval_seconds:($interval|tonumber),max_runtime_seconds:($max_runtime_seconds|tonumber),scope:"filesystem containing runner.temp"}' \
    > "$evidence/runner-metadata.json"
  uname -a > "$evidence/uname.txt"
  df -B1 -P "$runner_temp" > "$evidence/df-start-bytes.txt"
  df -i -P "$runner_temp" > "$evidence/df-start-inodes.txt"
}

sample_once() {
  local index="$1" start_epoch="$2" bytes_status=0 inodes_status=0
  local bytes inodes used_bytes used_inodes elapsed
  bytes="$(df -B1 -P "$runner_temp" 2>&1)" || bytes_status=$?
  inodes="$(df -i -P "$runner_temp" 2>&1)" || inodes_status=$?
  used_bytes="$(awk 'NR == 2 { print $3 }' <<< "$bytes")"
  used_inodes="$(awk 'NR == 2 { print $3 }' <<< "$inodes")"
  elapsed=$(( $(date +%s) - start_epoch ))
  jq -cn \
    --arg utc "$(utc_now)" --arg elapsed "$elapsed" \
    --arg bytes "$bytes" --arg bytes_status "$bytes_status" --arg used_bytes "$used_bytes" \
    --arg inodes "$inodes" --arg inodes_status "$inodes_status" --arg used_inodes "$used_inodes" \
    --arg device "$(awk 'NR == 2 { print $1 }' <<< "$bytes")" \
    --arg mount "$(awk 'NR == 2 { print $NF }' <<< "$bytes")" \
    --arg index "$index" \
    '{index:($index|tonumber),utc:$utc,elapsed_seconds:($elapsed|tonumber),device:$device,mount:$mount,df_bytes:$bytes,df_bytes_status:($bytes_status|tonumber),filesystem_used_bytes:(try ($used_bytes|tonumber) catch null),df_inodes:$inodes,df_inodes_status:($inodes_status|tonumber),filesystem_used_inodes:(try ($used_inodes|tonumber) catch null)}' \
    >> "$evidence/samples.jsonl"
  if [[ "$used_bytes" =~ ^[0-9]+$ ]] && (( used_bytes > max_used_bytes )); then max_used_bytes=$used_bytes; fi
  if [[ "$used_inodes" =~ ^[0-9]+$ ]] && (( used_inodes > max_used_inodes )); then max_used_inodes=$used_inodes; fi
}

add_root() {
  local name="$1" path="$2" escaped
  case "$path" in "$runner_temp"/*) ;; *) return ;; esac
  [[ -d "$path" && ! -L "$path" ]] || return
  escaped="$(escape_path "$path")"
  printf '%s\t%s\n' "$name" "$escaped" >> "$roots_file"
}

inventory_snapshot() {
  local snapshot_label="$1" roots_file="$evidence/roots-$1.tsv"
  local inventory_file="$evidence/inventory-$1.tsv"
  local summary_file="$evidence/inventory-summary-$1.tsv"
  local hashes_file="$evidence/content-hashes-$1.tsv"
  local root_name root_encoded root_path record dev ino nlink size blocks file escaped
  local count logical_sum allocated_sum allocated hash_bytes hash_cap_hit
  local status_count truncated inode
  : > "$roots_file"
  : > "$hashes_file"
  local cargo_home
  cargo_home="$(jq -r '.cargo_home // empty' "$evidence/runner-metadata.json")"
  [[ -z "$cargo_home" ]] || add_root cargo-home "$cargo_home"
  add_root bundle "$runner_temp/mbx-single-bundle"
  if [[ -f "$env_file" ]]; then
    while IFS='=' read -r key value; do
      case "$key" in
        MBX_CACHE_DIR|MBX_TARGET_ROOT|MBX_SHIMS_DIR) add_root "$key" "$value" ;;
      esac
    done < "$env_file"
  fi
  sort -u "$roots_file" -o "$roots_file"
  printf 'root_name\troot_path_percent_escaped\tfile_path_percent_escaped\tdevice\tinode\tlink_count\tlogical_bytes\tallocated_bytes_st_blocks_times_512\n' > "$inventory_file"
  printf 'root_name\troot_path_percent_escaped\tregular_files_observed\tlogical_bytes\tinode_allocated_bytes_sum_dedup_within_tree\tfile_cap\ttruncated\n' > "$summary_file"
  printf 'root_name\tsha256\tlogical_bytes\tfile_path_percent_escaped\n' > "$hashes_file"
  hash_bytes=0
  hash_cap_hit=false
  while IFS=$'\t' read -r root_name root_encoded; do
    [[ -n "$root_name" ]] || continue
    root_path="$(unescape_path "$root_encoded")"
    count=0
    logical_sum=0
    allocated_sum=0
    truncated=false
    declare -A seen_inodes=()
    while IFS=$'\t' read -r -d '' dev ino nlink size blocks file; do
      count=$((count + 1))
      if (( count > max_files )); then truncated=true; break; fi
      logical_sum=$((logical_sum + size))
      allocated=$((blocks * 512))
      inode="$dev:$ino"
      if [[ -z "${seen_inodes[$inode]+x}" ]]; then
        seen_inodes[$inode]=1
        allocated_sum=$((allocated_sum + allocated))
      fi
      escaped="$(escape_path "$file")"
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$root_name" "$root_encoded" "$escaped" "$dev" "$ino" "$nlink" "$size" "$allocated" >> "$inventory_file"
      if [[ "$snapshot_label" == export-complete && ( "$root_name" == bundle || "$root_name" == MBX_CACHE_DIR ) ]]; then
        if (( hash_bytes + size <= max_hash_bytes )); then
          hash="$(sha256sum -- "$file")"
          hash="${hash%% *}"
          printf '%s\t%s\t%s\t%s\n' "$root_name" "$hash" "$size" "$escaped" >> "$hashes_file"
          hash_bytes=$((hash_bytes + size))
        else
          hash_cap_hit=true
        fi
      fi
    done < <(find -P "$root_path" -type f -printf '%D\t%i\t%n\t%s\t%b\t%p\0' 2>> "$evidence/inventory-errors.txt")
    unset seen_inodes
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$root_name" "$root_encoded" "$((count > max_files ? max_files : count))" "$logical_sum" "$allocated_sum" "$max_files" "$truncated" >> "$summary_file"
  done < "$roots_file"
  if [[ "$snapshot_label" == export-complete ]]; then
    awk -F '\t' '$1 == "MBX_CACHE_DIR" { print $2 }' "$hashes_file" | sort -u > "$evidence/root-hashes-$snapshot_label.tsv"
    awk -F '\t' 'NR == FNR { roots[$1] = 1; next } $1 == "bundle" && roots[$2] { print $2 "\t" $3 "\t" $4 }' \
      "$evidence/root-hashes-$snapshot_label.tsv" "$hashes_file" > "$evidence/duplicate-content-$snapshot_label.tsv"
    local duplicate_count duplicate_bytes
    duplicate_count="$(wc -l < "$evidence/duplicate-content-$snapshot_label.tsv" | tr -d ' ')"
    duplicate_bytes="$(awk -F '\t' '{ total += $2 } END { printf "%.0f", total }' "$evidence/duplicate-content-$snapshot_label.tsv")"
    {
      printf 'interpretation\tequal SHA-256 content only; does not prove shared or duplicated physical extents\n'
      printf 'root_bundle_matching_file_count\t%s\n' "$duplicate_count"
      printf 'root_bundle_matching_logical_bytes\t%s\n' "$duplicate_bytes"
      printf 'hashed_logical_bytes\t%s\n' "$hash_bytes"
      printf 'hash_byte_cap\t%s\n' "$max_hash_bytes"
      printf 'hash_cap_reached\t%s\n' "$hash_cap_hit"
    } > "$evidence/duplicate-content-summary-$snapshot_label.tsv"
  fi
  {
    printf 'label\t%s\n' "$snapshot_label"
    printf 'utc\t%s\n' "$(utc_now)"
    printf 'inventory_file_cap_per_tree\t%s\n' "$max_files"
    printf 'allocated_interpretation\tsum st_blocks*512 once per dev/inode within each tree; reflink/COW extent sharing is unknown\n'
    printf 'filesystem_actual_pressure_source\tdf cadence rows in samples.jsonl and boundary df files; filesystem may have other users\n'
  } > "$evidence/inventory-meta-$snapshot_label.tsv"
  df -B1 -P "$runner_temp" > "$evidence/df-$snapshot_label-bytes.txt" 2>&1 || printf 'df_status=failed\n' >> "$evidence/df-$snapshot_label-bytes.txt"
  df -i -P "$runner_temp" > "$evidence/df-$snapshot_label-inodes.txt" 2>&1 || printf 'df_status=failed\n' >> "$evidence/df-$snapshot_label-inodes.txt"
}

if [[ "$mode" == snapshot ]]; then
  inventory_snapshot "$label"
  exit 0
fi

[[ "$mode" == sample ]] || { echo "unsupported sampler mode: $mode" >&2; exit 2; }
mkdir -p "$evidence"
write_metadata
start_epoch="$(date +%s)"
max_used_bytes=0
max_used_inodes=0
sample_count=0
while [[ ! -e "$evidence/sampler.stop" ]]; do
  sample_once "$sample_count" "$start_epoch"
  sample_count=$((sample_count + 1))
  if (( $(date +%s) - start_epoch >= 2700 )); then break; fi
  sleep "$interval"
done
sample_once "$sample_count" "$start_epoch"
sample_count=$((sample_count + 1))
jq -cn \
  --arg interval "$interval" --arg max_bytes "$max_used_bytes" --arg max_inodes "$max_used_inodes" \
  --arg count "$sample_count" --arg started "$(jq -r '.utc_started' "$evidence/runner-metadata.json")" \
  --arg ended "$(utc_now)" \
  '{sample_interval_seconds:($interval|tonumber),observed_max_filesystem_used_bytes_lower_bound:($max_bytes|tonumber),observed_max_filesystem_used_inodes_lower_bound:($max_inodes|tonumber),sample_count:($count|tonumber),observed_start_utc:$started,observed_end_utc:$ended,peak_is_instantaneous:false,used_values_include_other_filesystem_users:true,max_runtime_seconds:2700}' \
  > "$evidence/sampling-summary.json"
