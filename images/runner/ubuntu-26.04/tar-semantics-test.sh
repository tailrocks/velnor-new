#!/usr/bin/env bash
set -euo pipefail

tar_bin="${TAR_BIN:-tar}"
if ! "$tar_bin" --version | sed -n '1p' | grep -q '^tar (GNU tar) '; then
  echo "GNU tar is required." >&2
  exit 1
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/velnor-gnu-tar-semantics.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
export work tar_bin
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

case_pax_paths_and_metadata() {
  local root="$work/pax" src dest long deep i listed
  src="$root/src"
  dest="$root/dest"
  mkdir -p -- "$src/empty" "$dest"
  long=""
  for ((i = 0; i < 180; i++)); do long+="p"; done
  deep="file.txt"
  for ((i = 0; i < 30; i++)); do deep="dir/$deep"; done
  mkdir -p -- "$src/$(dirname -- "$deep")"
  printf 'long-ok\n' > "$src/$long"
  printf 'deep-ok\n' > "$src/$deep"
  printf 'run\n' > "$src/tool"
  printf 'data\n' > "$src/data"
  printf 'space-ok\n' > "$src/my file.txt"
  printf 'unicode-ok\n' > "$src/café.txt"
  mkdir -p -- "$src/dir-meta/child"
  printf 'dir-meta-ok\n' > "$src/dir-meta/child/file"
  chmod 0755 "$src/tool"
  chmod 0640 "$src/data"
  chmod 0750 "$src/dir-meta"
  touch -d @1234567890 "$src/tool" "$src/data"
  touch -d @1234567890 "$src/dir-meta"
  ln -s tool "$src/link"
  ln "$src/tool" "$src/alias"
  printf '%s\n' "$long" "$deep" tool data 'my file.txt' 'café.txt' \
    dir-meta link alias empty > "$root/manifest"
  "$tar_bin" --format=pax -cf "$root/archive.tar" -C "$src" \
    --files-from "$root/manifest"
  listed="$("$tar_bin" -tf "$root/archive.tar")"
  grep -Fxq -- "$long" <<< "$listed"
  grep -Fxq -- "$deep" <<< "$listed"
  "$tar_bin" --same-permissions -xf "$root/archive.tar" -C "$dest"
  grep -qx 'long-ok' "$dest/$long"
  grep -qx 'deep-ok' "$dest/$deep"
  grep -qx 'space-ok' "$dest/my file.txt"
  grep -qx 'unicode-ok' "$dest/café.txt"
  grep -qx 'dir-meta-ok' "$dest/dir-meta/child/file"
  [ "$(stat -c %a "$dest/tool")" = 755 ]
  [ "$(stat -c %a "$dest/data")" = 640 ]
  [ "$(stat -c %Y "$dest/tool")" = 1234567890 ]
  [ "$(stat -c %a "$dest/dir-meta")" = 750 ]
  [ "$(stat -c %Y "$dest/dir-meta")" = 1234567890 ]
  [ -d "$dest/empty" ]
  [ -z "$(find "$dest/empty" -mindepth 1 -print -quit)" ]
  [ "$(readlink "$dest/link")" = tool ]
  [ "$dest/tool" -ef "$dest/alias" ]
}

case_gzip_old_style_options() {
  local src="$work/gzip-src" dest="$work/gzip-dest"
  mkdir -p -- "$src" "$dest"
  printf 'gzip-ok\n' > "$src/payload"
  "$tar_bin" cfz "$work/legacy.tgz" -C "$src" payload
  "$tar_bin" xfz "$work/legacy.tgz" -C "$dest"
  grep -qx 'gzip-ok' "$dest/payload"
}

case_null_manifest_supports_newline_name() {
  local src="$work/newline-src" dest="$work/newline-dest" name
  name=$'line\nbreak.txt'
  mkdir -p -- "$src" "$dest"
  printf 'newline-ok\n' > "$src/$name"
  printf '%s\0' "$name" > "$work/newline.manifest"
  "$tar_bin" --format=pax --null -cf "$work/newline.tar" -C "$src" \
    --files-from "$work/newline.manifest"
  "$tar_bin" -xf "$work/newline.tar" -C "$dest"
  grep -qx 'newline-ok' "$dest/$name"
}

case_corrupt_header_fails() {
  mkdir -p -- "$work/corrupt-src"
  printf 'payload\n' > "$work/corrupt-src/file"
  "$tar_bin" -cf "$work/good.tar" -C "$work/corrupt-src" file
  cp -- "$work/good.tar" "$work/bad.tar"
  printf X | dd of="$work/bad.tar" bs=1 seek=0 conv=notrunc status=none
  if "$tar_bin" -tf "$work/bad.tar" > "$work/corrupt.log" 2>&1; then
    return 1
  fi
}

case_truncated_member_fails() {
  mkdir -p -- "$work/truncated-src" "$work/truncated-dest"
  dd if=/dev/zero of="$work/truncated-src/file" bs=4096 count=1 status=none
  "$tar_bin" -cf "$work/full.tar" -C "$work/truncated-src" file
  head -c 2048 "$work/full.tar" > "$work/truncated.tar"
  if "$tar_bin" -xf "$work/truncated.tar" -C "$work/truncated-dest" \
    > "$work/truncated.log" 2>&1; then
    return 1
  fi
}

run_case pax-long-path-links-and-metadata case_pax_paths_and_metadata
run_case gzip-round-trip-and-legacy-options case_gzip_old_style_options
run_case null-manifest-supports-newline-name case_null_manifest_supports_newline_name
run_case corrupt-header-fails case_corrupt_header_fails
run_case truncated-member-fails case_truncated_member_fails
printf 'RESULT pass=%s fail=%s\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
