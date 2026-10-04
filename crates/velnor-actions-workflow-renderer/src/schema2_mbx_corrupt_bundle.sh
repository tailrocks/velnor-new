#!/usr/bin/env bash
set -euo pipefail
umask 077
export LC_ALL=C

bundle="$1"
evidence="$2"
runner_temp="$3"
max_files=20000
max_hash_bytes=$((1024 * 1024 * 1024))
bundle_identity=''
selected_state=''
selected_digest=''
eligible_file=''
mutated_offset=''
ancestor_paths=()
ancestor_ids=()
temp_path=''
temp_basename=''
parent_fd=''
parent_identity=''
walk_path=''

cleanup_temp() {
  if [[ -n "$walk_path" && ( -e "$walk_path" || -L "$walk_path" ) ]]; then rm -- "$walk_path"; fi
  if [[ -n "$temp_basename" && -n "$parent_fd" ]]; then
    (cd -- "/proc/$$/fd/$parent_fd" && rm -f -- "$temp_basename") || true
  elif [[ -n "$temp_path" && ( -e "$temp_path" || -L "$temp_path" ) ]]; then
    rm -- "$temp_path"
  fi
}
trap cleanup_temp EXIT

escape_path() {
  local value="$1"
  value="${value//%/%25}"
  value="${value//$'\t'/%09}"
  value="${value//$'\n'/%0A}"
  value="${value//$'\r'/%0D}"
  printf '%s' "$value"
}

validate_bundle() {
  local evidence_real runner_real bundle_real evidence_owner bundle_owner bundle_mode marker identity expected_marker
  [[ -d "$evidence" && ! -L "$evidence" ]] || return 1
  evidence_real="$(realpath -e -- "$evidence")" || return 1
  [[ "$evidence_real" == "$evidence" ]] || return 1
  runner_real="$(realpath -e -- "$runner_temp")" || return 1
  [[ "$runner_real" == "$runner_temp" ]] || return 1
  [[ -d "$bundle" && ! -L "$bundle" ]] || return 1
  bundle_real="$(realpath -e -- "$bundle")" || return 1
  [[ "$bundle_real" == "$bundle" && "$bundle" == "$runner_real/mbx-single-bundle" ]] || return 1
  [[ "$(stat -c '%d' -- "$bundle")" == "$(stat -c '%d' -- "$evidence")" ]] || return 1
  evidence_owner="$(stat -c '%u:%g:%a' -- "$evidence")" || return 1
  [[ "$evidence_owner" == "$(id -u):$(id -g):700" ]] || return 1
  [[ -f "$evidence/private.marker" && ! -L "$evidence/private.marker" && \
    "$(stat -c '%h:%a:%u:%g' -- "$evidence/private.marker")" == "1:600:$(id -u):$(id -g)" ]] || return 1
  marker="$(cat -- "$evidence/private.marker")" || return 1
  expected_marker="$(printf 'mbx-cache-evidence-v1\t%s\t%s\t%s' \
    "${GITHUB_RUN_ID-}" "${GITHUB_RUN_ATTEMPT-}" "${MBX_QUALIFICATION_JOB_ID-}")"
  [[ "$marker" == "$expected_marker" ]] || return 1
  [[ -f "$evidence/private.identity" && ! -L "$evidence/private.identity" && \
    "$(stat -c '%h:%a:%u:%g' -- "$evidence/private.identity")" == "1:600:$(id -u):$(id -g)" ]] || return 1
  identity="$(cat -- "$evidence/private.identity")" || return 1
  [[ "$identity" == "$(stat -c '%d:%i:%u:%g' -- "$evidence")" ]] || return 1
  bundle_owner="$(stat -c '%u:%g' -- "$bundle")" || return 1
  [[ "$bundle_owner" == "$(id -u):$(id -g)" ]] || return 1
  bundle_mode="$(stat -c '%a' -- "$bundle")" || return 1
  [[ "$bundle_mode" =~ ^[0-7]{3,4}$ ]] || return 1
  (( (8#$bundle_mode & 0022) == 0 )) || return 1
  bundle_identity="$(stat -c '%d:%i' -- "$bundle")"
}

validate_private_directory() {
  local path="$1" owner mode
  [[ -d "$path" && ! -L "$path" && "$(realpath -e -- "$path")" == "$path" ]] || return 1
  owner="$(stat -c '%u:%g' -- "$path")" || return 1
  [[ "$owner" == "$(id -u):$(id -g)" ]] || return 1
  mode="$(stat -c '%a' -- "$path")" || return 1
  [[ "$mode" =~ ^[0-7]{3,4}$ ]] || return 1
  (( (8#$mode & 0022) == 0 ))
}

validate_bundle_identity() {
  [[ -d "$bundle" && ! -L "$bundle" && "$(realpath -e -- "$bundle")" == "$bundle" ]] || return 1
  [[ "$(stat -c '%d:%i' -- "$bundle")" == "$bundle_identity" ]]
}

capture_ancestors() {
  local parent="$1" relative segment current
  local -a segments=()
  ancestor_paths=("$bundle")
  ancestor_ids=("$(stat -c '%d:%i' -- "$bundle")")
  relative="${parent#"$bundle"/}"
  [[ "$relative" != "$parent" && "$relative" != *'..'* ]] || return 1
  IFS='/' read -r -a segments <<< "$relative"
  current="$bundle"
  for segment in "${segments[@]}"; do
    [[ -n "$segment" && "$segment" != . && "$segment" != .. ]] || return 1
    current="$current/$segment"
    [[ -d "$current" && ! -L "$current" && "$(realpath -e -- "$current")" == "$current" ]] || return 1
    validate_private_directory "$current" || return 1
    ancestor_paths+=("$current")
    ancestor_ids+=("$(stat -c '%d:%i' -- "$current")")
  done
  [[ "$current" == "$parent" ]]
}

validate_ancestors() {
  local index path
  validate_bundle_identity || return 1
  for index in "${!ancestor_paths[@]}"; do
    path="${ancestor_paths[$index]}"
    [[ -d "$path" && ! -L "$path" && "$(realpath -e -- "$path")" == "$path" ]] || return 1
    [[ "$(stat -c '%d:%i' -- "$path")" == "${ancestor_ids[$index]}" ]] || return 1
    validate_private_directory "$path" || return 1
  done
}

fingerprint_bundle() {
  local label="$1" count=0 logical_sum=0
  local manifest="$evidence/corrupt-bundle-$label.tsv"
  local dev ino nlink size blocks file before after relative escaped digest walk_file walk_status=0 hash_output
  local link_probe link_status=0
  [[ ! -e "$manifest" && ! -L "$manifest" ]] || return 1
  validate_bundle_identity || { echo 'bundle identity changed before fingerprint' >&2; return 1; }
  printf 'relative_path_percent_escaped\tdevice\tinode\tlink_count\tlogical_bytes\tallocated_bytes_st_blocks_times_512\tsha256\n' > "$manifest"
  walk_file="$evidence/corrupt-walk-$label.nul"
  walk_path="$walk_file"
  [[ ! -e "$walk_file" && ! -L "$walk_file" ]] || return 1
  (set -o noclobber; : > "$walk_file") || return 1
  if find -P "$bundle" -type f -printf '%D\0%i\0%n\0%s\0%b\0%p\0' \
      2>> "$evidence/corrupt-find-errors.txt" | head -z -n "$(((max_files + 1) * 6))" > "$walk_file"; then
    walk_status=0
  else
    walk_status=$?
  fi
  while IFS= read -r -d '' dev && IFS= read -r -d '' ino && IFS= read -r -d '' nlink && \
    IFS= read -r -d '' size && IFS= read -r -d '' blocks && IFS= read -r -d '' file; do
    count=$((count + 1))
    (( count <= max_files )) || { echo 'corrupt probe file cap exceeded' >&2; return 1; }
    logical_sum=$((logical_sum + size))
    (( logical_sum <= max_hash_bytes )) || { echo 'corrupt probe hash byte cap exceeded' >&2; return 1; }
    [[ -f "$file" && ! -L "$file" && "$(realpath -e -- "$file")" == "$file" ]] || return 1
    before="$(stat -c '%d:%i:%h:%s:%b:%a:%u:%g:%Y:%F' -- "$file")"
    if ! hash_output="$(sha256sum -- "$file")"; then
      echo 'payload fingerprint hash failed' >&2
      return 1
    fi
    digest="${hash_output%% *}"
    [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || { echo 'payload fingerprint hash malformed' >&2; return 1; }
    after="$(stat -c '%d:%i:%h:%s:%b:%a:%u:%g:%Y:%F' -- "$file")"
    [[ "$before" == "$after" ]] || { echo 'bundle changed during fingerprint' >&2; return 1; }
    relative="${file#"$bundle"/}"
    escaped="$(escape_path "$relative")"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$escaped" "$dev" "$ino" "$nlink" "$size" "$((blocks * 512))" "$digest" >> "$manifest"
  done < "$walk_file"
  rm -- "$walk_file"
  walk_path=''
  if (( walk_status != 0 )); then
    printf 'bundle find failed with status %s\n' "$walk_status" >&2
    return 1
  fi
  (( count <= max_files )) || { echo 'corrupt probe file cap exceeded' >&2; return 1; }
  [[ ! -s "$evidence/corrupt-find-errors.txt" ]] || return 1
  validate_bundle_identity || return 1
  local symlink_present=false
  link_probe="$evidence/corrupt-symlink-probe.nul"
  [[ ! -e "$link_probe" && ! -L "$link_probe" ]] || return 1
  (set -o noclobber; : > "$link_probe") || return 1
  if find -P "$bundle" -type l -print0 -quit > "$link_probe" 2>> "$evidence/corrupt-find-errors.txt"; then
    link_status=0
  else
    link_status=$?
  fi
  (( link_status == 0 )) || { rm -- "$link_probe"; return 1; }
  [[ ! -s "$link_probe" ]] || symlink_present=true
  rm -- "$link_probe"
  [[ ! -s "$evidence/corrupt-find-errors.txt" ]] || return 1
  {
    printf 'regular_file_count\t%s\n' "$count"
    printf 'symlink_present_not_followed\t%s\n' "$symlink_present"
    printf 'logical_bytes\t%s\n' "$logical_sum"
    printf 'hash_byte_cap\t%s\n' "$max_hash_bytes"
    printf 'format_parsing\tnone; opaque paths and SHA-256 only\n'
  } > "$evidence/corrupt-bundle-$label-summary.tsv"
}

choose_payload() {
  local candidate canonical parent state escaped candidates_file find_status=0
  eligible_file=''
  candidates_file="$evidence/corrupt-candidate.nul"
  [[ ! -e "$candidates_file" && ! -L "$candidates_file" ]] || return 1
  (set -o noclobber; : > "$candidates_file") || return 1
  if find -P "$bundle" -type f -links 1 -size +0c -path '*/cas/v1/blake3/*/*-*' -print0 -quit \
      > "$candidates_file" 2>> "$evidence/corrupt-find-errors.txt"; then
    find_status=0
  else
    find_status=$?
  fi
  (( find_status == 0 )) || { rm -- "$candidates_file"; return 1; }
  if IFS= read -r -d '' candidate < "$candidates_file"; then eligible_file="$candidate"; fi
  rm -- "$candidates_file"
  [[ ! -s "$evidence/corrupt-find-errors.txt" ]] || return 1
  [[ -n "$eligible_file" ]] || { echo 'no nonempty single-link CAS payload to corrupt' >&2; return 1; }
  canonical="$(realpath -e -- "$eligible_file")" || return 1
  [[ "$canonical" == "$eligible_file" && ! -L "$eligible_file" && -f "$eligible_file" ]] || return 1
  parent="${eligible_file%/*}"
  capture_ancestors "$parent" || return 1
  validate_ancestors || return 1
  state="$(stat -c '%d:%i:%h:%s:%b:%a:%u:%g:%F' -- "$eligible_file")"
  [[ "${state##*:}" == 'regular file' ]] || return 1
  [[ "$(stat -c '%h' -- "$eligible_file")" == 1 ]] || return 1
  [[ "$(stat -c '%u:%g' -- "$eligible_file")" == "$(id -u):$(id -g)" ]] || return 1
  selected_state="$state"
  escaped="$(escape_path "${eligible_file#"$bundle"/}")"
  selected_digest="$(awk -F '\t' -v path="$escaped" '$1 == path { print $7 }' "$evidence/corrupt-bundle-before.tsv")"
  [[ "$selected_digest" =~ ^[0-9a-f]{64}$ ]] || return 1
}

replace_one_byte_atomically() {
  local parent temp target_device temp_device current_state size offset original_byte changed_byte replacement
  local source_fd temp_fd source_fd_identity temp_fd_identity temp_fd_path source_fd_path source_mode current_digest
  local eligible_basename final_parent_identity final_state
  parent="${eligible_file%/*}"
  validate_ancestors || return 1
  validate_private_directory "$parent" || return 1
  parent_identity="$(stat -c '%d:%i' -- "$parent")" || return 1
  exec {parent_fd}< "$parent"
  final_parent_identity="$(stat -Lc '%d:%i:%F' -- "/proc/$$/fd/$parent_fd")" || return 1
  [[ "$final_parent_identity" == "$parent_identity:directory" ]] || return 1
  [[ -z "$(ps -C mbx -o pid= 2>/dev/null | tr -d '[:space:]')" ]] || {
    echo 'an MBX process is still active during payload mutation' >&2
    return 1
  }
  target_device="$(stat -c '%d' -- "$eligible_file")"
  temp="$(mktemp --tmpdir="/proc/$$/fd/$parent_fd" .mbx-corrupt.XXXXXXXXXX)"
  temp_path="$temp"
  temp_basename="${temp##*/}"
  eligible_basename="${eligible_file##*/}"
  [[ -f "$temp" && ! -L "$temp" && "$(stat -Lc '%d' -- "$temp")" == "$target_device" ]] || return 1
  exec {source_fd}< "$eligible_file"
  source_fd_path="/proc/$$/fd/$source_fd"
  source_fd_identity="$(stat -Lc '%d:%i:%h:%s:%b:%a:%u:%g:%F' -- "$source_fd_path")"
  [[ "$source_fd_identity" == "$selected_state" ]] || return 1
  if ! current_digest="$(sha256sum -- "$source_fd_path")"; then return 1; fi
  [[ "${current_digest%% *}" == "$selected_digest" ]] || return 1
  [[ "$(stat -c '%d:%i:%h:%s:%b:%a:%u:%g:%F' -- "$eligible_file")" == "$selected_state" ]] || return 1
  exec {temp_fd}<> "$temp"
  temp_fd_path="/proc/$$/fd/$temp_fd"
  temp_fd_identity="$(stat -Lc '%d:%i' -- "$temp_fd_path")"
  [[ "$temp_fd_identity" == "$(stat -c '%d:%i' -- "$temp")" ]] || return 1
  cat <&"$source_fd" >&"$temp_fd"
  current_state="$(stat -c '%d:%i:%h:%s:%b:%a:%u:%g:%F' -- "$eligible_file")"
  [[ "$current_state" == "$selected_state" && "$(realpath -e -- "$eligible_file")" == "$eligible_file" ]] || return 1
  [[ -f "$temp" && ! -L "$temp" && "$(stat -c '%d:%i' -- "$temp")" == "$temp_fd_identity" ]] || return 1
  [[ "$(stat -Lc '%h' -- "$temp_fd_path")" == 1 ]] || return 1
  size="$(stat -Lc '%s' -- "$temp_fd_path")"
  [[ "$size" == "$(stat -c '%s' -- "$eligible_file")" ]] || return 1
  (( size > 0 && size <= max_hash_bytes )) || return 1
  offset=$((size / 2))
  original_byte="$(od -An -tu1 -N1 -j "$offset" -- "$temp_fd_path" | tr -d '[:space:]')"
  [[ "$original_byte" =~ ^[0-9]{1,3}$ ]] || return 1
  changed_byte=$((original_byte ^ 255))
  printf -v replacement '\\%03o' "$changed_byte"
  printf '%b' "$replacement" | dd of="$temp_fd_path" bs=1 seek="$offset" conv=notrunc status=none
  source_mode="$(stat -Lc '%a' -- "$source_fd_path")"
  exec {source_fd}<&-
  chmod "$source_mode" -- "$temp_fd_path"
  sync -d -- "$temp_fd_path"
  temp_device="$(stat -Lc '%d' -- "$temp_fd_path")"
  [[ "$temp_device" == "$target_device" && -f "$temp" && ! -L "$temp" ]] || return 1
  current_state="$(stat -c '%d:%i:%h:%s:%b:%a:%u:%g:%F' -- "$eligible_file")"
  [[ "$current_state" == "$selected_state" && "$(realpath -e -- "$eligible_file")" == "$eligible_file" ]] || return 1
  [[ "$(stat -c '%d:%i' -- "$temp")" == "$temp_fd_identity" && \
    "$(stat -Lc '%d:%i:%h:%s:%b:%a:%u:%g:%F' -- "$temp_fd_path")" == \
    "$(stat -c '%d:%i:%h:%s:%b:%a:%u:%g:%F' -- "$temp")" ]] || return 1
  validate_ancestors || return 1
  printf '%s\n' "$eligible_file" > "$evidence/mutated-target-path.txt"
  printf '%s\n' "$selected_state" > "$evidence/mutated-target-before-stat.txt"
  (
    cd -- "/proc/$$/fd/$parent_fd"
    [[ "$(stat -c '%d:%i' .)" == "$parent_identity" ]] || exit 1
    validate_ancestors || exit 1
    [[ ! -L "./$eligible_basename" && "$(stat -c '%d:%i:%h:%s:%b:%a:%u:%g:%F' -- "./$eligible_basename")" == "$selected_state" ]] || exit 1
    [[ ! -L "./$temp_basename" && "$(stat -c '%d:%i' -- "./$temp_basename")" == "$temp_fd_identity" ]] || exit 1
    mv -T -- "./$temp_basename" "./$eligible_basename"
  ) || return 1
  temp_path=''
  temp_basename=''
  sync -d -- "$temp_fd_path"
  exec {temp_fd}>&-
  local after_state
  final_state="$(cd -- "/proc/$$/fd/$parent_fd" && stat -c '%d:%i:%h:%s:%b:%a:%u:%g:%F' -- "./$eligible_basename")" || return 1
  [[ "$(cd -- "/proc/$$/fd/$parent_fd" && stat -c '%d:%h:%s:%b:%a:%u:%g:%F' -- "./$eligible_basename")" == \
     "$(cut -d: -f1,3,4,5,6,7,8,9 -- "$evidence/mutated-target-before-stat.txt")" ]] || return 1
  after_state="$final_state"
  printf '%s\n' "$after_state" > "$evidence/mutated-target-after-stat.txt"
  mutated_offset="$offset"
}

validate_bundle
mkdir "$evidence/corrupt-mutation.lock"
runner_temp_identity="$(stat -c '%d:%i' -- "$runner_temp")"
bundle_owner_mode="$(stat -c '%u:%g:%a' -- "$bundle")"
(
  set -o noclobber
  printf 'pid\t%s\nrun_id\t%s\nrun_attempt\t%s\njob_id\t%s\nmode\tsequential hosted job-local bundle mutation\nrunner_temp_path\t%s\nrunner_temp_device_inode\t%s\nbundle_path\t%s\nbundle_device_inode\t%s\nbundle_owner_mode\t%s\nrestore_match\t%s\nrestore_primary\t%s\nactive_mbx_processes\t0\nnamespace_bound\topen parent directory descriptor for atomic relative rename; post-mutation identity/fingerprint failure rejects any namespace move\n' \
    "$$" "${GITHUB_RUN_ID-}" "${GITHUB_RUN_ATTEMPT-}" "${MBX_QUALIFICATION_JOB_ID-}" \
    "$runner_temp" "$runner_temp_identity" "$bundle" "$bundle_identity" "$bundle_owner_mode" "$MATCHED" "$PRIMARY" \
    > "$evidence/corrupt-mutation.lock/owner.tsv"
)
test ! -e "$evidence/corrupt-find-errors.txt"
test ! -L "$evidence/corrupt-find-errors.txt"
(
  set -o noclobber
  : > "$evidence/corrupt-find-errors.txt"
)
fingerprint_bundle before
choose_payload
test ! -e "$evidence/mutated-target-path.txt"
replace_one_byte_atomically
fingerprint_bundle after
relative_changed="${eligible_file#"$bundle"/}"
escaped_changed="$(escape_path "$relative_changed")"
before_digest="$(awk -F '\t' -v path="$escaped_changed" '$1 == path { print $7 }' "$evidence/corrupt-bundle-before.tsv")"
after_digest="$(awk -F '\t' -v path="$escaped_changed" '$1 == path { print $7 }' "$evidence/corrupt-bundle-after.tsv")"
[[ -n "$before_digest" && -n "$after_digest" && "$before_digest" != "$after_digest" ]] || {
  echo 'selected payload fingerprint did not change' >&2
  exit 1
}
if ! awk -F '\t' -v target="$escaped_changed" '
  NR == FNR { if (FNR > 1) { dev[$1]=$2; ino[$1]=$3; links[$1]=$4; size[$1]=$5; alloc[$1]=$6; digest[$1]=$7; seen[$1]=1; before_count++ } next }
  FNR > 1 {
    if (!($1 in seen) || dev[$1]!=$2 || links[$1]!=$4 || size[$1]!=$5 || alloc[$1]!=$6) bad=1
    if ($1 == target) { if (ino[$1] == $3 || digest[$1] == $7) bad=1; target_count++ }
    else if (ino[$1]!=$3 || digest[$1]!=$7) bad=1
    delete seen[$1]
    after_count++
  }
  END { for (path in seen) bad=1; if (bad || before_count!=after_count || target_count!=1) exit 1 }
' "$evidence/corrupt-bundle-before.tsv" "$evidence/corrupt-bundle-after.tsv"; then
  echo 'bundle changed outside one same-size payload replacement' >&2
  exit 1
fi
cmp -s "$evidence/corrupt-bundle-before-summary.tsv" "$evidence/corrupt-bundle-after-summary.tsv" || {
  echo 'bundle regular-file or symlink count changed' >&2
  exit 1
}
{
  printf 'mutated_relative_path_percent_escaped\t%s\n' "$escaped_changed"
  printf 'mutated_byte_offset\t%s\n' "$mutated_offset"
  printf 'mutated_regular_file_count\t1\n'
  printf 'mutation_method\tprivate same-filesystem temp, same-size byte flip, atomic rename replacement\n'
  printf 'followed_symlink\tfalse; find -P plus canonical/lstat checks\n'
  printf 'tree_concurrency\tper-job RUNNER_TEMP tree; restore step ended; importer/build/export not started; active MBX process count zero\n'
  printf 'namespace_containment\tparent directory opened and identity checked before cwd-relative atomic rename; any later pathname identity or fingerprint change rejects mutation receipt\n'
} > "$evidence/corrupt-mutation.tsv"
