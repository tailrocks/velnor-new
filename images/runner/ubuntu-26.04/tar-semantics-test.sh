#!/usr/bin/env bash
set -euo pipefail

tar_bin="${TAR_BIN:-/usr/bin/tar.gnu}"
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

make_hardlink_escape_archive() {
  python3 - "$1" "$2" <<'PY'
import io
import sys
import tarfile

archive, outside = sys.argv[1:]
with tarfile.open(archive, "w", format=tarfile.PAX_FORMAT) as output:
    target = tarfile.TarInfo("target")
    target.size = 6
    output.addfile(target, io.BytesIO(b"inside\n"))

    safe = tarfile.TarInfo("safe-link")
    safe.type = tarfile.LNKTYPE
    safe.linkname = "target"
    output.addfile(safe)

    hostile = tarfile.TarInfo("hostile-link")
    hostile.type = tarfile.LNKTYPE
    hostile.linkname = outside
    output.addfile(hostile)
PY
}

case_default_parent_hardlink_target_stays_bounded() {
  local root="$work/parent-hardlink" outside status=0
  outside="$root/outside/target"
  mkdir -p -- "$root/dest" "$(dirname -- "$outside")"
  printf 'outside\n' > "$outside"
  make_hardlink_escape_archive "$root/archive.tar" ../outside/target
  printf 'hostile-link\n' > "$root/selection"
  "$tar_bin" -xf "$root/archive.tar" -C "$root/dest" \
    --files-from "$root/selection" > "$root/extract.log" 2>&1 || status=$?
  [ "$status" -ne 0 ]
  [ ! -e "$root/dest/hostile-link" ] || \
    [ ! "$root/dest/hostile-link" -ef "$outside" ]
  grep -qx 'outside' "$outside"
}

case_unselected_hostile_hardlink_does_not_block_safe_links() {
  local root="$work/unselected-hardlink" outside
  outside="$root/outside/target"
  mkdir -p -- "$root/dest" "$(dirname -- "$outside")"
  printf 'outside\n' > "$outside"
  make_hardlink_escape_archive "$root/archive.tar" "$outside"
  printf 'target\nsafe-link\n' > "$root/selection"
  "$tar_bin" -xPf "$root/archive.tar" -C "$root/dest" \
    --files-from "$root/selection"
  grep -qx 'inside' "$root/dest/target"
  [ "$root/dest/target" -ef "$root/dest/safe-link" ]
  [ ! -e "$root/dest/hostile-link" ]
  grep -qx 'outside' "$outside"
}

case_failed_decompressor_is_reaped() {
  local root="$work/decoder-failure" src helper pid_file archive status=0 pid
  local i state cmd still_running=1
  src="$root/src"
  helper="$root/failing-decoder"
  pid_file="$root/decoder.pid"
  archive="$root/archive.tar.gz"
  mkdir -p -- "$src" "$root/dest" "$root/scratch"
  printf 'payload\n' > "$src/payload"
  "$tar_bin" -czf "$archive" -C "$src" payload
  cat > "$helper" <<'SH'
#!/bin/sh
printf '%s\n' "$$" > "$VELNOR_TEST_DECODER_PID"
cat >/dev/null
exit 23
SH
  chmod 0755 "$helper"
  export VELNOR_TEST_DECODER_PID="$pid_file"
  if TMPDIR="$root/scratch" timeout --kill-after=1s 5s "$tar_bin" \
    --use-compress-program "$helper" -tf "$archive" \
    > "$root/list.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -ne 0 ] && [ "$status" -ne 124 ] && [ "$status" -ne 137 ]
  [ -s "$pid_file" ]
  pid="$(cat "$pid_file")"
  for ((i = 0; i < 100; i++)); do
    if [ ! -r "/proc/$pid/stat" ]; then
      still_running=0
      break
    fi
    state="$(awk '{ print $3 }' "/proc/$pid/stat")"
    [ "$state" != Z ] || return 1
    cmd="$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null || true)"
    case "$cmd" in
      *"$helper"*) sleep 0.01 ;;
      *) still_running=0; break ;;
    esac
  done
  [ "$still_running" -eq 0 ]
  # The PID file is test bookkeeping; scratch below records tar-created residue.
  [ -z "$(find "$root/scratch" -type p -print -quit)" ]
  [ -z "$(find "$root/scratch" -mindepth 1 -print -quit)" ]
}

case_early_list_consumer_is_bounded() {
  local root="$work/list-consumer" src long i status=0 pid_file pid
  local state cmd still_running=1
  src="$root/src"
  mkdir -p -- "$src" "$root/scratch"
  long=""
  for ((i = 0; i < 180; i++)); do long+="p"; done
  printf 'member\n' > "$src/$long"
  : > "$root/manifest"
  for ((i = 0; i < 600; i++)); do printf '%s\n' "$long" >> "$root/manifest"; done
  "$tar_bin" --format=pax -cf "$root/archive.tar" -C "$src" \
    --files-from "$root/manifest"
  pid_file="$root/tar.pid"
  if TMPDIR="$root/scratch" timeout --kill-after=1s 5s bash -c \
    'set -o pipefail; (printf "%s\n" "$BASHPID" > "$3"; exec "$1" -tf "$2") | head -n 1 >/dev/null' \
    _ "$tar_bin" "$root/archive.tar" "$pid_file" > "$root/list.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -ne 0 ] && [ "$status" -ne 124 ] && [ "$status" -ne 137 ]
  [ -s "$pid_file" ]
  pid="$(cat "$pid_file")"
  for ((i = 0; i < 100; i++)); do
    if [ ! -r "/proc/$pid/stat" ]; then
      still_running=0
      break
    fi
    state="$(awk '{ print $3 }' "/proc/$pid/stat")"
    [ "$state" != Z ] || return 1
    cmd="$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null || true)"
    case "$cmd" in
      *"$tar_bin"*) sleep 0.01 ;;
      *) still_running=0; break ;;
    esac
  done
  [ "$still_running" -eq 0 ]
  [ -z "$(find "$root/scratch" -type p -print -quit)" ]
  [ -z "$(find "$root/scratch" -mindepth 1 -print -quit)" ]
}

case_bounded_write_failure_preserves_unrelated_state() {
  local root="$work/write-failure" src dest dir status=0
  src="$root/src"
  dest="$root/dest"
  dir="$src/restricted"
  mkdir -p -- "$dir" "$dest/restricted" "$root/scratch"
  printf 'first member\n' > "$dir/first"
  dd if=/dev/zero of="$dir/large" bs=4096 count=1 status=none
  chmod 0750 "$dir"
  touch -d @1234567890 "$dir"
  "$tar_bin" --format=pax -cf "$root/archive.tar" -C "$src" restricted
  chmod 0700 "$dest/restricted"
  touch -d @1000000000 "$dest/restricted"
  printf 'keep this file\n' > "$dest/unrelated"
  # This RLIMIT_FSIZE failure is an EFBIG surrogate, not filesystem ENOSPC.
  if TMPDIR="$root/scratch" timeout --kill-after=1s 5s bash -c \
    'ulimit -c 0; ulimit -f 2; exec "$1" --same-permissions -xf "$2" -C "$3"' \
    _ "$tar_bin" "$root/archive.tar" "$dest" > "$root/extract.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  [ "$status" -ne 0 ] && [ "$status" -ne 124 ] && [ "$status" -ne 137 ]
  grep -qx 'keep this file' "$dest/unrelated"
  [ -z "$(find "$root/scratch" -type p -print -quit)" ]
  [ -z "$(find "$root/scratch" -mindepth 1 -print -quit)" ]
}

case_stream_enospc_is_reported() {
  local root="$work/stream-enospc" src status=0
  src="$root/src"
  mkdir -p -- "$src"
  dd if=/dev/zero of="$src/payload" bs=4096 count=1 status=none
  "$tar_bin" -cf "$root/archive.tar" -C "$src" payload
  # /dev/full checks streamed-output ENOSPC; it does not simulate extract ENOSPC.
  if "$tar_bin" -xOf "$root/archive.tar" payload > /dev/full 2> "$root/full.log"; then
    status=0
  else
    status=$?
  fi
  [ "$status" -ne 0 ]
}

run_case pax-long-path-links-and-metadata case_pax_paths_and_metadata
run_case gzip-round-trip-and-legacy-options case_gzip_old_style_options
run_case null-manifest-supports-newline-name case_null_manifest_supports_newline_name
run_case corrupt-header-fails case_corrupt_header_fails
run_case truncated-member-fails case_truncated_member_fails
run_case default-parent-hardlink-target-stays-bounded case_default_parent_hardlink_target_stays_bounded
run_case unselected-hostile-hardlink-does-not-block-safe-links case_unselected_hostile_hardlink_does_not_block_safe_links
run_case failed-decompressor-is-reaped case_failed_decompressor_is_reaped
run_case early-list-consumer-is-bounded case_early_list_consumer_is_bounded
run_case bounded-write-failure-preserves-unrelated-state case_bounded_write_failure_preserves_unrelated_state
run_case stream-enospc-is-reported case_stream_enospc_is_reported
printf 'RESULT pass=%s fail=%s\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
