#!/usr/bin/env bash
set -euo pipefail

tar_bin="${TAR_BIN:-/usr/bin/tar.gnu}"
if ! "$tar_bin" --version | sed -n '1p' | grep -q '^tar (GNU tar) '; then
  echo "GNU tar is required." >&2
  exit 1
fi
command -v zstdmt >/dev/null
command -v unzstd >/dev/null

work="$(mktemp -d "${TMPDIR:-/tmp}/velnor-gnu-tar-cache.XXXXXX")"
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

# Match the Linux GNU tar commands in the pinned actions/cache bundle.
case_cache_parent_paths_and_sdk_compression() {
  local home="$work/home/runner"
  local source="$work/home/runner/_work/repo/repo"
  local restore="$work/restore/home/runner/_work/repo/repo"
  local safe_restore="$work/safe-restore/home/runner/_work/repo/repo"
  local long manifest archive gzip_archive status=0 safe_status=0
  long=""
  for ((i = 0; i < 90; i++)); do long+="n"; done
  mkdir -p -- "$source" "$home/.cache/mbx/$long/sub" "$restore" "$safe_restore"
  printf 'mbx-ok\n' > "$home/.cache/mbx/marker"
  printf 'long-ok\n' > "$home/.cache/mbx/$long/sub/file"
  printf 'stay\n' > "$source/keep.txt"
  manifest="$work/manifest.txt"
  printf '%s\n' '../../../.cache/mbx/marker' \
    "../../../.cache/mbx/$long/sub/file" keep.txt > "$manifest"

  archive="$work/cache.tzst"
  (
    cd "$work"
    "$tar_bin" --posix -cf cache.tzst --exclude cache.tzst -P \
      -C "$source" --files-from manifest.txt \
      --use-compress-program zstdmt
    "$tar_bin" -tf cache.tzst -P --use-compress-program unzstd \
      > "$work/list"
  )
  grep -Fxq '../../../.cache/mbx/marker' "$work/list"
  grep -Fxq "../../../.cache/mbx/$long/sub/file" "$work/list"

  "$tar_bin" -xf "$archive" -P -C "$restore" \
    --use-compress-program unzstd
  grep -qx 'mbx-ok' "$work/restore/home/runner/.cache/mbx/marker"
  grep -qx 'long-ok' "$work/restore/home/runner/.cache/mbx/$long/sub/file"
  grep -qx 'stay' "$restore/keep.txt"
  [ ! -e "$restore/.cache" ]

  "$tar_bin" -xf "$archive" -C "$safe_restore" > "$work/default.log" 2>&1 || safe_status=$?
  [ "$safe_status" -eq 2 ]
  [ ! -e "$safe_restore/.cache/mbx/marker" ]
  [ ! -e "$work/safe-restore/home/runner/.cache/mbx/marker" ]
  grep -Fq "Member name contains '..'" "$work/default.log"
  grep -qx 'stay' "$safe_restore/keep.txt"

  gzip_archive="$work/cache.tgz"
  (
    cd "$work"
    "$tar_bin" --posix -cf cache.tgz --exclude cache.tgz -P \
      -C "$source" --files-from manifest.txt -z
  )
  mkdir -p -- "$work/gzip-restore/home/runner/_work/repo/repo"
  "$tar_bin" -xf "$gzip_archive" -P \
    -C "$work/gzip-restore/home/runner/_work/repo/repo" -z
  grep -qx 'long-ok' "$work/gzip-restore/home/runner/.cache/mbx/$long/sub/file"
}

case_cache_path_collision_order() {
  local order="$1" root="$work/collision-$1"
  local home="$work/collision-$1/home/runner"
  local source="$work/collision-$1/home/runner/_work/repo/repo"
  local restore="$work/collision-$1/restore/home/runner/_work/repo/repo"
  local manifest="$work/collision-$1/manifest.txt"
  mkdir -p -- "$source/.cache" "$home/.cache" "$restore"
  printf 'workspace-cache\n' > "$source/.cache/x"
  printf 'home-cache\n' > "$home/.cache/x"
  if [ "$order" = inside-first ]; then
    printf '%s\n' .cache/x ../../../.cache/x > "$manifest"
  else
    printf '%s\n' ../../../.cache/x .cache/x > "$manifest"
  fi
  "$tar_bin" -cf "$root/cache.tar" -P -C "$source" \
    --files-from "$manifest"
  "$tar_bin" -xf "$root/cache.tar" -P -C "$restore"
  grep -qx 'workspace-cache' "$restore/.cache/x"
  grep -qx 'home-cache' "$work/collision-$1/restore/home/runner/.cache/x"
}

case_cache_collision_inside_first() { case_cache_path_collision_order inside-first; }
case_cache_collision_home_first() { case_cache_path_collision_order home-first; }

case_repeated_parent_member_last_wins() {
  local root="$work/repeated-parent" one="$work/repeated-parent/one"
  local two="$work/repeated-parent/two" dest="$work/repeated-parent/restore/a/b/c"
  mkdir -p -- "$one/a/b/c" "$one/.cache" "$two/a/b/c" "$two/.cache" "$dest"
  printf 'first\n' > "$one/.cache/x"
  printf 'second\n' > "$two/.cache/x"
  "$tar_bin" --posix -cf "$root/cache.tar" -P \
    -C "$one/a/b/c" ../../../.cache/x \
    -C "$two/a/b/c" ../../../.cache/x
  "$tar_bin" -xf "$root/cache.tar" -P -C "$dest"
  grep -qx 'second' "$root/restore/.cache/x"
  [ "$(find "$root/restore" -type f | wc -l | tr -d ' ')" = 1 ]
  [ ! -e "$dest/.cache/x" ]
}

case_gnu_zstd_long_program() {
  local src="$work/zstd-long-src" dest="$work/zstd-long-dest"
  mkdir -p -- "$src" "$dest"
  printf 'zstd-long-ok\n' > "$src/payload"
  printf 'payload\n' > "$work/zstd-long-manifest.txt"
  (
    cd "$work"
    "$tar_bin" --posix -cf zstd-long.tzst --exclude zstd-long.tzst \
      -C "$src" --files-from zstd-long-manifest.txt \
      --use-compress-program 'zstdmt --long=30'
  )
  "$tar_bin" -xf "$work/zstd-long.tzst" -C "$dest" \
    --use-compress-program 'unzstd --long=30'
  grep -qx 'zstd-long-ok' "$dest/payload"
}

case_absolute_name_requires_explicit_p() {
  local root="$work/absolute" file default_dest restored status=0
  root="$work/absolute"
  file="$root/member.txt"
  default_dest="$work/absolute-default"
  restored="$work/absolute-restored"
  mkdir -p -- "$root" "$default_dest" "$restored"
  printf 'absolute-ok\n' > "$file"
  "$tar_bin" -cf "$work/absolute.tar" -P "$file"
  rm -f -- "$file"
  "$tar_bin" -xf "$work/absolute.tar" -C "$default_dest" \
    > "$work/absolute-default.log" 2>&1 || status=$?
  [ "$status" -le 2 ]
  [ ! -e "$file" ]
  [ "$(cat "$default_dest/${file#/}")" = absolute-ok ]
  "$tar_bin" -xf "$work/absolute.tar" -P -C "$restored"
  grep -qx 'absolute-ok' "$file"
}

run_case cache-parent-paths-pax-and-sdk-compression case_cache_parent_paths_and_sdk_compression
run_case cache-path-collision-inside-first case_cache_collision_inside_first
run_case cache-path-collision-home-first case_cache_collision_home_first
run_case repeated-parent-member-last-wins case_repeated_parent_member_last_wins
run_case gnu-zstd-long-external-program case_gnu_zstd_long_program
run_case absolute-name-requires-explicit-p case_absolute_name_requires_explicit_p
printf 'RESULT pass=%s fail=%s\n' "$passed" "$failed"
[ "$failed" -eq 0 ]
