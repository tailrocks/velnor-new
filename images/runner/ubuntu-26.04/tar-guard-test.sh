#!/usr/bin/env bash
set -euo pipefail

guard_tar=/usr/bin/tar
native_tar=/usr/bin/tar.gnu
work="$(mktemp -d "${TMPDIR:-/tmp}/velnor-tar-guard-test.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
export guard_tar native_tar work
passed=0
failed=0

run_case() {
  local name="$1" test_function="$2" function_source
  function_source="$(declare -f)"
  if bash -euo pipefail -c "$function_source"$'\n'"$test_function"; then
    passed=$((passed + 1))
    printf 'PASS %s\n' "$name"
  else
    failed=$((failed + 1))
    printf 'FAIL %s\n' "$name" >&2
  fi
}

make_symlink_archive() {
  python3 - "$1" "$2" <<'PY'
import io
import sys
import tarfile

archive, target = sys.argv[1:]
with tarfile.open(archive, "w", format=tarfile.PAX_FORMAT) as output:
    link = tarfile.TarInfo("link")
    link.type = tarfile.SYMTYPE
    link.linkname = target
    output.addfile(link)

    child = tarfile.TarInfo("link/payload")
    child.size = 7
    output.addfile(child, io.BytesIO(b"unsafe\n"))

    safe = tarfile.TarInfo("safe.txt")
    safe.size = 5
    output.addfile(safe, io.BytesIO(b"safe\n"))
PY
}

make_standalone_symlink_archive() {
  python3 - "$1" "$2" <<'PY'
import sys
import tarfile

archive, target = sys.argv[1:]
with tarfile.open(archive, "w", format=tarfile.PAX_FORMAT) as output:
    link = tarfile.TarInfo("link")
    link.type = tarfile.SYMTYPE
    link.linkname = target
    output.addfile(link)
PY
}

case_tar_options_cannot_enable_absolute_names() {
  local root="$work/tar-options" status=0 relative_member
  mkdir -p -- "$root/src" "$root/dest"
  printf 'archive-payload\n' >"$root/src/member"
  "$native_tar" -P -cf "$root/absolute.tar" -- "$root/src/member"
  printf 'outside-sentinel\n' >"$root/src/member"
  relative_member="${root#/}/src/member"
  if TAR_OPTIONS=-P "$guard_tar" -xf "$root/absolute.tar" -C "$root/dest" \
      >"$root/extract.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -eq 0 ]
  grep -qx 'archive-payload' "$root/dest/$relative_member"
  grep -qx 'outside-sentinel' "$root/src/member"
}

case_double_dash_keeps_default_absolute_name_stripping() {
  local root="$work/double-dash-absolute" relative_member
  mkdir -p -- "$root/src" "$root/dest"
  printf 'absolute-payload\n' >"$root/src/member"
  "$native_tar" -P -cf "$root/absolute.tar" -- "$root/src/member"
  relative_member="${root#/}/src/member"
  "$guard_tar" -xf "$root/absolute.tar" -C "$root/dest" --
  grep -qx 'absolute-payload' "$root/dest/$relative_member"
}

case_dashless_fxP_cluster_is_preflighted() {
  local root="$work/dashless-fxP" status=0
  mkdir -p -- "$root/dest" "$root/outside"
  make_symlink_archive "$root/archive.tar" "$root/outside"
  if (cd "$root/dest" && "$guard_tar" fxP "$root/archive.tar") \
      >"$root/extract.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -eq 2 ]
  [ ! -e "$root/outside/payload" ]
  [ ! -e "$root/dest/link" ]
}

case_archive_symlink_cannot_extract_selected_child() {
  local root="$work/archive-symlink" status=0
  mkdir -p -- "$root/dest" "$root/outside"
  make_symlink_archive "$root/archive.tar" ../outside
  if "$guard_tar" -xPf "$root/archive.tar" -C "$root/dest" \
      >"$root/extract.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -eq 2 ]
  [ ! -e "$root/outside/payload" ]
  [ ! -e "$root/dest/link" ]
}

case_preexisting_symlink_cannot_extract_child() {
  local root="$work/preexisting-symlink" status=0
  mkdir -p -- "$root/dest" "$root/outside"
  ln -s ../outside "$root/dest/link"
  python3 - "$root/archive.tar" <<'PY'
import io
import sys
import tarfile

with tarfile.open(sys.argv[1], "w", format=tarfile.PAX_FORMAT) as output:
    item = tarfile.TarInfo("link/payload")
    item.size = 7
    output.addfile(item, io.BytesIO(b"unsafe\n"))
PY
  if "$guard_tar" -xPf "$root/archive.tar" -C "$root/dest" \
      >"$root/extract.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -eq 2 ]
  [ ! -e "$root/outside/payload" ]
  [ -L "$root/dest/link" ]
}

case_absolute_symlink_target_outside_roots_is_rejected() {
  local root="$work/absolute-link" status=0
  mkdir -p -- "$root/dest" "$root/outside"
  make_symlink_archive "$root/archive.tar" "$root/outside"
  if "$guard_tar" --extract --file="$root/archive.tar" \
      --directory="$root/dest" --absolute-names \
      >"$root/extract.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -eq 2 ]
  [ ! -e "$root/dest/link" ]
}

case_unselected_symlink_does_not_block_selected_safe_member() {
  local root="$work/unselected-link"
  mkdir -p -- "$root/dest" "$root/outside"
  make_symlink_archive "$root/archive.tar" ../outside
  printf 'safe.txt\n' >"$root/selection"
  "$guard_tar" -xf "$root/archive.tar" -C "$root/dest" \
    --files-from "$root/selection"
  grep -qx safe "$root/dest/safe.txt"
  [ ! -e "$root/dest/link" ]
  [ ! -e "$root/outside/payload" ]
}

case_relative_external_cache_symlink_is_allowed_with_p() {
  local root="$work/relative-cache"
  mkdir -p -- "$root/dest" "$root/outside"
  make_standalone_symlink_archive "$root/archive.tar" ../outside
  printf 'link\n' >"$root/selection"
  "$guard_tar" -xPf "$root/archive.tar" -C "$root/dest" \
    --files-from "$root/selection"
  [ -L "$root/dest/link" ]
  [ "$(readlink "$root/dest/link")" = ../outside ]
  [ ! -e "$root/outside/payload" ]
}

case_selected_symlink_prefix_child_is_rejected() {
  local root="$work/selected-symlink-prefix" status=0
  mkdir -p -- "$root/dest" "$root/outside"
  make_symlink_archive "$root/archive.tar" ../outside
  printf 'link\n' >"$root/selection"
  if "$guard_tar" -xPf "$root/archive.tar" -C "$root/dest" \
      --files-from "$root/selection" >"$root/extract.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -eq 2 ]
  [ ! -e "$root/outside/payload" ]
  [ ! -e "$root/dest/link" ]
  [ ! -e "$root/dest/safe.txt" ]
}

case_unselected_absolute_header_does_not_block_selected_p_member() {
  local root="$work/unselected-absolute-header" absolute_member
  mkdir -p -- "$root/dest"
  absolute_member="$root/unselected.txt"
  python3 - "$root/archive.tar" "$absolute_member" <<'PY'
import io
import sys
import tarfile

archive, absolute_member = sys.argv[1:]
with tarfile.open(archive, "w", format=tarfile.PAX_FORMAT) as output:
    item = tarfile.TarInfo(absolute_member)
    item.size = 8
    output.addfile(item, io.BytesIO(b"outside\n"))

    safe = tarfile.TarInfo("safe.txt")
    safe.size = 5
    output.addfile(safe, io.BytesIO(b"safe\n"))
PY
  printf 'safe.txt\n' >"$root/selection"
  "$guard_tar" -xPf "$root/archive.tar" -C "$root/dest" \
    --files-from "$root/selection"
  grep -qx safe "$root/dest/safe.txt"
  [ ! -e "$absolute_member" ]
}

case_selected_absolute_member_stays_in_synthetic_tree() {
  local root="$work/selected-absolute-member" absolute_member
  mkdir -p -- "$root/dest"
  absolute_member="$root/dest/selected.txt"
  python3 - "$root/archive.tar" "$absolute_member" <<'PY'
import io
import sys
import tarfile

archive, absolute_member = sys.argv[1:]
with tarfile.open(archive, "w", format=tarfile.PAX_FORMAT) as output:
    item = tarfile.TarInfo(absolute_member)
    item.size = 9
    output.addfile(item, io.BytesIO(b"selected\n"))
PY
  printf '%s\n' "$absolute_member" >"$root/selection"
  "$guard_tar" -xPf "$root/archive.tar" -C "$root/dest" \
    --files-from "$root/selection"
  grep -qx selected "$absolute_member"
}

case_in_root_symlink_and_target_are_allowed_with_p() {
  local root="$work/in-root-link"
  mkdir -p -- "$root/dest"
  python3 - "$root/archive.tar" <<'PY'
import io
import sys
import tarfile

with tarfile.open(sys.argv[1], "w", format=tarfile.PAX_FORMAT) as output:
    link = tarfile.TarInfo("link")
    link.type = tarfile.SYMTYPE
    link.linkname = "target"
    output.addfile(link)

    target = tarfile.TarInfo("target")
    target.size = 7
    output.addfile(target, io.BytesIO(b"inside\n"))
PY
  "$guard_tar" -xPf "$root/archive.tar" -C "$root/dest"
  [ -L "$root/dest/link" ]
  [ "$(readlink "$root/dest/link")" = target ]
  grep -qx inside "$root/dest/target"
}

case_guarded_p_files_from_names_are_literal_and_large_list_survives() {
  local root="$work/files-from" src dest manifest name carriage_name i
  src="$root/src"
  dest="$root/dest"
  carriage_name=$'carriage\r'
  mkdir -p -- "$src" "$dest"
  printf 'leading\n' >"$src/-leading"
  printf 'backslash\n' >"$src/back\\slash"
  printf 'space\n' >"$src/space name"
  printf 'bracket\n' >"$src/literal[1]"
  printf 'hash\n' >"$src/#literal"
  printf 'carriage\n' >"$src/$carriage_name"
  name=""
  for ((i = 0; i < 200; i++)); do name+="l"; done
  printf 'long\n' >"$src/$name"
  "$native_tar" -cf "$root/archive.tar" -C "$src" -- \
    -leading 'back\slash' 'space name' 'literal[1]' '#literal' \
    "$carriage_name" "$name"
  : >"$root/selected"
  printf '%s\n' '-leading' 'back\slash' 'space name' 'literal[1]' \
    '#literal' "$carriage_name" >>"$root/selected"
  "$guard_tar" -xPf "$root/archive.tar" -C "$dest" --files-from "$root/selected"
  grep -qx leading "$dest/-leading"
  grep -qx backslash "$dest/back\\slash"
  grep -qx space "$dest/space name"
  grep -qx bracket "$dest/literal[1]"
  grep -qx hash "$dest/#literal"
  grep -qx carriage "$dest/$carriage_name"
  [ ! -e "$dest/$name" ]

  manifest="$root/large-selection"
  : >"$manifest"
  for ((i = 0; i < 11000; i++)); do printf '%s\n' "$name" >>"$manifest"; done
  [ "$(wc -c <"$manifest" | tr -d ' ')" -gt 2097152 ]
  "$guard_tar" -xPf "$root/archive.tar" -C "$dest" --files-from "$manifest"
  grep -qx long "$dest/$name"
}

case_native_backend_accepts_unique_large_files_from_list() {
  local root="$work/native-unique-large-list" dest count component prefix file_prefix i
  dest="$root/dest"
  mkdir -p -- "$dest"
  python3 - "$root/archive.tar" "$root/unique.list" <<'PY'
import io
import sys
import tarfile

archive, manifest = sys.argv[1:]
prefix='/'.join(['d'*200 for _ in range(10)])
with tarfile.open(archive, "w", format=tarfile.PAX_FORMAT) as output, open(manifest, "wb") as names:
    payload=b'unique\n'
    for index in range(1000):
        name=prefix+'/'+'u'*182+f'{index:08d}'
        item=tarfile.TarInfo(name)
        item.size=len(payload)
        item.mode=0o644
        output.addfile(item, io.BytesIO(payload))
        names.write(name.encode()+b'\n')
PY
  [ "$(wc -c <"$root/unique.list" | tr -d ' ')" -gt 2097152 ]
  "$native_tar" -xP --directory "$dest" --file "$root/archive.tar" \
    --no-wildcards --anchored --verbatim-files-from \
    --files-from "$root/unique.list"
  count="$(find "$dest" -type f -print | wc -l | tr -d ' ')"
  [ "$count" -eq 1000 ]
  component="$(printf 'd%.0s' {1..200})"
  prefix="$component"
  for ((i = 1; i < 10; i++)); do prefix+="/$component"; done
  file_prefix="$(printf 'u%.0s' {1..182})"
  grep -qx unique "$dest/$prefix/${file_prefix}00000000"
  grep -qx unique "$dest/$prefix/${file_prefix}00000999"
}

case_literal_p_after_double_dash_is_a_filename() {
  local root="$work/literal-P" src dest
  src="$root/src"
  dest="$root/dest"
  mkdir -p -- "$src" "$dest"
  printf 'literal\n' >"$src/-P"
  "$native_tar" -cf "$root/archive.tar" -C "$src" -- -P
  "$guard_tar" -xf "$root/archive.tar" -C "$dest" -- -P
  grep -qx literal "$dest/-P"
}

case_compressed_archive_is_detected_without_option() {
  local root="$work/auto-compressed" src dest
  src="$root/src"
  dest="$root/dest"
  mkdir -p -- "$src" "$dest"
  printf 'compressed\n' >"$src/member"
  "$native_tar" -czf "$root/archive.tgz" -C "$src" member
  "$guard_tar" -xf "$root/archive.tgz" -C "$dest"
  grep -qx compressed "$dest/member"
}

case_compress_program_receives_decompress_flag() {
  local root="$work/compress-program" src dest
  src="$root/src"
  dest="$root/dest"
  mkdir -p -- "$src" "$dest"
  printf 'program\n' >"$src/member"
  "$native_tar" -czf "$root/archive.tar.gz" -C "$src" member
  "$guard_tar" -xf "$root/archive.tar.gz" -C "$dest" \
    --use-compress-program gzip
  grep -qx program "$dest/member"
}

case_relative_paths_use_invocation_directory_for_archive_and_selection() {
  local root="$work/relative-paths" src dest_before dest_after
  src="$root/src"
  dest_before="$root/dest-before"
  dest_after="$root/dest-after"
  mkdir -p -- "$src" "$dest_before" "$dest_after"
  printf 'position\n' >"$src/member"
  printf 'decoy\n' >"$src/other"
  printf 'member\n' >"$root/selection"
  printf 'other\n' >"$dest_before/selection"
  "$native_tar" -cf "$root/archive.tar" -C "$src" member
  "$native_tar" -cf "$dest_before/archive.tar" -C "$src" other
  (
    cd "$root"
    "$guard_tar" -C dest-before -xPf archive.tar --files-from selection
  )
  grep -qx position "$dest_before/member"
  [ ! -e "$dest_before/other" ]
  (
    cd "$root"
    "$guard_tar" -xPf archive.tar -C dest-after
  )
  grep -qx position "$dest_after/member"
  [ ! -e "$dest_after/other" ]
}

case_attached_short_values_keep_directory_and_file_positions() {
  local root="$work/attached-short-values" src dest intermediate
  src="$root/src"
  dest="$root/intermediate/dest"
  intermediate="$root/intermediate"
  mkdir -p -- "$src" "$dest"
  printf 'attached\n' >"$src/member"
  printf 'decoy\n' >"$src/other"
  "$native_tar" -cf "$root/archive.tar" -C "$src" member
  "$native_tar" -cf "$intermediate/archive.tar" -C "$src" other
  printf 'member\n' >"$root/selection"
  printf 'other\n' >"$intermediate/selection"
  (
    cd "$root"
    "$guard_tar" -Cintermediate -xPfarchive.tar -Cdest -Tselection
  )
  grep -qx attached "$dest/member"
  [ ! -e "$dest/other" ]
}

case_ambiguous_arity_and_option_prefixes_fail_closed() {
  local root="$work/ambiguous-arity"
  mkdir -p -- "$root/dest" "$root/outside" "$root/--"
  make_symlink_archive "$root/archive.tar" ../outside

  refuse_without_escape() {
    local name="$1" status=0
    shift
    if (cd -- "$root" && "$guard_tar" "$@") >"$root/$name.log" 2>&1; then
      status=0
    else
      status=$?
    fi
    [ "$status" -eq 2 ]
    grep -q '^velnor-tar:' "$root/$name.log"
    [ ! -e "$root/outside/payload" ]
    [ ! -e "$root/dest/link" ]
    [ ! -e "$root/--/link" ]
  }

  refuse_without_escape unknown-value-double-dash-without-operation \
    --unknown --
  grep -q 'ambiguous option arity' \
    "$root/unknown-value-double-dash-without-operation.log"
  refuse_without_escape unmatched-dash-double-dash - --
  grep -q 'ambiguous option arity' "$root/unmatched-dash-double-dash.log"
  refuse_without_escape unknown-value-double-dash \
    --unknown -- -xP -f "$root/archive.tar" -C "$root/dest"
  grep -q 'ambiguous option arity' "$root/unknown-value-double-dash.log"
  refuse_without_escape exclude-value-double-dash \
    --exclude -- -xP -f "$root/archive.tar" -C "$root/dest"
  refuse_without_escape directory-alias-double-dash \
    --dir -- -xP -f "$root/archive.tar"
  refuse_without_escape abbreviated-extract-and-absolute \
    --extr -P -f "$root/archive.tar" -C "$root/dest"
  refuse_without_escape abbreviated-absolute-name \
    -x --absol -f "$root/archive.tar" -C "$root/dest"
  refuse_without_escape file-value-before-extract-and-absolute \
    -f "$root/archive.tar" -xP -C "$root/dest"
}

case_nonabsolute_files_from_preserves_native_gnu_option_lines() {
  local root="$work/native-files-from" src destination
  src="$root/src"
  destination="$root/from-list"
  mkdir -p -- "$src" "$destination"
  printf 'native-list\n' >"$src/member"
  "$native_tar" -cf "$root/archive.tar" -C "$src" -- member
  printf '%s\n' "-C$destination" member >"$root/selection"
  (
    cd "$root"
    "$guard_tar" -xf archive.tar -T selection
  )
  grep -qx native-list "$destination/member"
  [ ! -e "$root/member" ]
}

case_guard_workspace_is_removed_and_backend_status_preserved() {
  local root="$work/backend-cleanup" status=0 native_status=0 name
  mkdir -p -- "$root/src" "$root/dest" "$root/direct-dest" "$root/scratch"
  printf 'backend-ok\n' >"$root/src/member"
  "$native_tar" -cf "$root/success.tar" -C "$root/src" member
  if TMPDIR="$root/scratch" "$guard_tar" -xPf "$root/success.tar" \
      -C "$root/dest" >"$root/success.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -eq 0 ]
  grep -qx backend-ok "$root/dest/member"
  [ -z "$(find "$root/scratch" -mindepth 1 -print -quit)" ]

  name="$(printf '%0300d' 0 | tr '0' n)"
  python3 - "$root/failure.tar" "$name" <<'PY'
import io
import sys
import tarfile

with tarfile.open(sys.argv[1], "w", format=tarfile.PAX_FORMAT) as output:
    item = tarfile.TarInfo(sys.argv[2])
    item.size = 8
    output.addfile(item, io.BytesIO(b"too-long"))
PY
  if env -u TAR_OPTIONS LC_ALL=C "$native_tar" -xPf "$root/failure.tar" \
      -C "$root/direct-dest" >"$root/direct-failure.log" 2>&1; then
    native_status=0
  else
    native_status=$?
  fi
  [ "$native_status" -ne 0 ] && [ "$native_status" -ne 124 ] && [ "$native_status" -ne 137 ]
  status=0
  if TMPDIR="$root/scratch" LC_ALL=C "$guard_tar" -xPf "$root/failure.tar" \
      -C "$root/dest" >"$root/failure.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -eq "$native_status" ]
  grep -Fq 'File name too long' "$root/failure.log"
  [ -z "$(find "$root/scratch" -mindepth 1 -print -quit)" ]
}

case_directory_after_member_selector_is_rejected() {
  local root="$work/late-directory" status=0
  mkdir -p -- "$root/dest" "$root/src"
  printf 'late\n' >"$root/src/member"
  "$native_tar" -cf "$root/archive.tar" -C "$root/src" member
  if (
    cd "$root"
    "$guard_tar" -xPf archive.tar member -C dest
  ) >"$root/extract.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -eq 2 ]
  [ ! -e "$root/member" ]
  [ ! -e "$root/dest/member" ]
}

run_case tar-options-cannot-enable-absolute-names case_tar_options_cannot_enable_absolute_names
run_case double-dash-keeps-default-absolute-name-stripping case_double_dash_keeps_default_absolute_name_stripping
run_case dashless-fxP-cluster-is-preflighted case_dashless_fxP_cluster_is_preflighted
run_case archive-symlink-cannot-extract-selected-child case_archive_symlink_cannot_extract_selected_child
run_case preexisting-symlink-cannot-extract-child case_preexisting_symlink_cannot_extract_child
run_case absolute-symlink-target-outside-roots-is-rejected case_absolute_symlink_target_outside_roots_is_rejected
run_case unselected-symlink-does-not-block-selected-safe-member case_unselected_symlink_does_not_block_selected_safe_member
run_case relative-external-cache-symlink-is-allowed-with-p case_relative_external_cache_symlink_is_allowed_with_p
run_case selected-symlink-prefix-child-is-rejected case_selected_symlink_prefix_child_is_rejected
run_case unselected-absolute-header-does-not-block-selected-p-member case_unselected_absolute_header_does_not_block_selected_p_member
run_case selected-absolute-member-stays-in-synthetic-tree case_selected_absolute_member_stays_in_synthetic_tree
run_case in-root-symlink-and-target-are-allowed-with-p case_in_root_symlink_and_target_are_allowed_with_p
run_case guarded-p-files-from-names-are-literal case_guarded_p_files_from_names_are_literal_and_large_list_survives
run_case native-backend-accepts-unique-large-files-from-list case_native_backend_accepts_unique_large_files_from_list
run_case literal-p-after-double-dash-is-a-filename case_literal_p_after_double_dash_is_a_filename
run_case compressed-archive-is-detected-without-option case_compressed_archive_is_detected_without_option
run_case compress-program-receives-decompress-flag case_compress_program_receives_decompress_flag
run_case relative-archive-and-selection-paths-use-invocation-directory case_relative_paths_use_invocation_directory_for_archive_and_selection
run_case attached-short-values-keep-directory-and-file-positions case_attached_short_values_keep_directory_and_file_positions
run_case ambiguous-arity-and-option-prefixes-fail-closed case_ambiguous_arity_and_option_prefixes_fail_closed
run_case nonabsolute-files-from-preserves-native-gnu-option-lines case_nonabsolute_files_from_preserves_native_gnu_option_lines
run_case guard-workspace-is-removed-and-backend-status-preserved case_guard_workspace_is_removed_and_backend_status_preserved
run_case directory-after-member-selector-is-rejected case_directory_after_member_selector_is_rejected
printf 'RESULT pass=%s fail=%s\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
