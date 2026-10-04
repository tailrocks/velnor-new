#!/usr/bin/env bash
set -euo pipefail

bundle="$1"
evidence="$2"
max_files=20000
max_hash_bytes=$((4 * 1024 * 1024 * 1024))

escape_path() {
  local value="$1"
  value="${value//%/%25}"
  value="${value//$'\t'/%09}"
  value="${value//$'\n'/%0A}"
  value="${value//$'\r'/%0D}"
  printf '%s' "$value"
}

fingerprint_bundle() {
  local label="$1" count=0 logical_sum=0 record dev ino nlink size blocks file relative escaped digest
  local file_list="$evidence/corrupt-files-$label.nul"
  local manifest="$evidence/corrupt-bundle-$label.tsv"
  find -P "$bundle" -type f -printf '%D\t%i\t%n\t%s\t%b\t%p\0' > "$file_list"
  printf 'relative_path_percent_escaped\tdevice\tinode\tlink_count\tlogical_bytes\tallocated_bytes_st_blocks_times_512\tsha256\n' > "$manifest"
  while IFS=$'\t' read -r -d '' dev ino nlink size blocks file; do
    count=$((count + 1))
    if (( count > max_files )); then echo 'corrupt probe file cap exceeded' >&2; return 1; fi
    logical_sum=$((logical_sum + size))
    if (( logical_sum > max_hash_bytes )); then echo 'corrupt probe hash byte cap exceeded' >&2; return 1; fi
    relative="${file#"$bundle"/}"
    escaped="$(escape_path "$relative")"
    digest="$(sha256sum -- "$file")"
    digest="${digest%% *}"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$escaped" "$dev" "$ino" "$nlink" "$size" "$((blocks * 512))" "$digest" >> "$manifest"
  done < "$file_list"
  local symlink_count
  symlink_count="$(find -P "$bundle" -type l -printf x | wc -c | tr -d ' ')"
  {
    printf 'regular_file_count\t%s\n' "$count"
    printf 'symlink_count_not_followed\t%s\n' "$symlink_count"
    printf 'logical_bytes\t%s\n' "$logical_sum"
    printf 'hash_byte_cap\t%s\n' "$max_hash_bytes"
    printf 'format_parsing\tnone; opaque paths and SHA-256 only\n'
  } > "$evidence/corrupt-bundle-$label-summary.tsv"
  rm -f -- "$file_list"
}

fingerprint_bundle before
eligible_file=''
while IFS= read -r -d '' candidate; do
  eligible_file="$candidate"
  break
done < <(find -P "$bundle" -type f -links 1 -size +0c -path '*/cas/v1/blake3/*/*-*' -print0 | sort -z)
if [[ -z "$eligible_file" ]]; then echo 'no nonempty single-link regular payload to corrupt' >&2; exit 1; fi
size="$(stat -c '%s' -- "$eligible_file")"
offset=$((size / 2))
original_byte="$(od -An -tu1 -N1 -j "$offset" -- "$eligible_file" | tr -d '[:space:]')"
if [[ ! "$original_byte" =~ ^[0-9]+$ ]]; then echo 'could not read selected payload byte' >&2; exit 1; fi
changed_byte=$((original_byte ^ 255))
printf -v replacement '\\%03o' "$changed_byte"
printf '%b' "$replacement" | dd of="$eligible_file" bs=1 seek="$offset" conv=notrunc status=none
sync -d "$eligible_file"
fingerprint_bundle after
relative_changed="${eligible_file#"$bundle"/}"
escaped_changed="$(escape_path "$relative_changed")"
before_digest="$(awk -F '\t' -v path="$escaped_changed" '$1 == path { print $7 }' "$evidence/corrupt-bundle-before.tsv")"
after_digest="$(awk -F '\t' -v path="$escaped_changed" '$1 == path { print $7 }' "$evidence/corrupt-bundle-after.tsv")"
if [[ -z "$before_digest" || -z "$after_digest" || "$before_digest" == "$after_digest" ]]; then
  echo 'selected payload fingerprint did not change' >&2
  exit 1
fi
changed_count="$(awk -F '\t' 'NR == FNR { before[$1] = $7; next } $1 in before && before[$1] != $7 { changed++ } END { print changed + 0 }' \
  "$evidence/corrupt-bundle-before.tsv" "$evidence/corrupt-bundle-after.tsv")"
if [[ "$changed_count" != 1 ]]; then echo 'corruption changed an unexpected regular-file set' >&2; exit 1; fi
if ! awk -F '\t' '
  NR == FNR { if (FNR > 1) { dev[$1]=$2; ino[$1]=$3; links[$1]=$4; size[$1]=$5; alloc[$1]=$6; digest[$1]=$7; seen[$1]=1; before_count++ } next }
  FNR > 1 {
    if (!($1 in seen) || dev[$1]!=$2 || ino[$1]!=$3 || links[$1]!=$4 || size[$1]!=$5 || alloc[$1]!=$6) bad=1
    if ($1 in seen && digest[$1]!=$7) changed++
    delete seen[$1]
    after_count++
  }
  END { for (path in seen) bad=1; if (bad || before_count!=after_count || changed!=1) exit 1 }
' "$evidence/corrupt-bundle-before.tsv" "$evidence/corrupt-bundle-after.tsv"; then
  echo 'bundle inventory changed outside one same-size payload byte mutation' >&2
  exit 1
fi
{
  printf 'mutated_relative_path_percent_escaped\t%s\n' "$escaped_changed"
  printf 'mutated_byte_offset\t%s\n' "$offset"
  printf 'mutated_regular_file_count\t%s\n' "$changed_count"
} > "$evidence/corrupt-mutation.tsv"
