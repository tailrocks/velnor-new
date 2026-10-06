# Additional tar-shim CLI and streaming regressions.

stream_extract() {
  local stage="$1" root="$2" limit="$3"
  shift 3
  local hit="${stage}.hit"
  local done="${stage}.done"
  local watcher status=0
  rm -rf -- "$stage"
  mkdir -p -- "$stage"
  : >"$hit"
  rm -f -- "$done"
  (
    while [ ! -f "$done" ]; do
      find "$stage" -type f -size +$((limit - 1))c -print >>"$hit" 2>/dev/null || true
      find "$stage" -type f -name 'velnor-tar-raw*' -print >>"$hit" 2>/dev/null || true
      sleep 0.05
    done
  ) &
  watcher=$!
  PATH="$root/bin:$PATH" VELNOR_TAR_WATCH="$hit" VELNOR_TAR_LIMIT="$limit" TMPDIR="$stage" \
    bash "$shim" "$@" || status=$?
  touch "$done"
  wait "$watcher" || true
  [ "$status" -eq 0 ] || return 1
  if [ -s "$hit" ]; then
    cat "$hit" >&2
    return 1
  fi
  [ -z "$(find "$stage" -mindepth 1 -print -quit)" ] || return 1
}

# `tar -cf -C dir -- -dash-member` must not reach BusyBox argv.
case_dash_positional() {
  local root="$work/dashpos"
  local listed
  rm -rf -- "$root"
  mkdir -p "$root/src" "$root/out"
  printf payload >"$root/src/-dash-member"
  printf ok >"$root/src/plain"
  bash "$shim" -cf "$root/arc.tar" -C "$root/src" -- plain -dash-member || return 1
  listed="$(tar.gnu t -f "$root/arc.tar")"
  [ "$listed" = $'plain\n-dash-member' ] || return 1
  bash "$shim" -xf "$root/arc.tar" -C "$root/out" || return 1
  cmp -s "$root/src/plain" "$root/out/plain" || return 1
  cmp -s "$root/src/-dash-member" "$root/out/-dash-member" || return 1
}

case_dash_attached_values() {
  local root="$work/dash-attached" plain gnu_plain old old_cluster gnu_old_cluster
  rm -rf -- "$root"
  mkdir -p "$root/cache" "$root/out"
  printf 'attached-ok\n' >"$root/cache/payload"
  (
    cd "$root" || return 1
    bash "$shim" -cfarchive.tar -Ccache payload || return 1
    tar.gnu -cfgnu-attached.tar -Ccache payload || return 1
    bash "$shim" c -fafter-c.tar -Ccache payload || return 1
    tar.gnu c -fgnu-after-c.tar -Ccache payload || return 1
    bash "$shim" cfz oldstyle.tar -Ccache payload || return 1
    bash "$shim" cfCz oldstyle-cluster.tar cache payload || return 1
    tar.gnu cfCz gnu-oldstyle-cluster.tar cache payload || return 1
  ) || return 1
  [ -f "$root/archive.tar" ] || return 1
  [ -f "$root/gnu-attached.tar" ] || return 1
  [ -f "$root/after-c.tar" ] || return 1
  [ -f "$root/gnu-after-c.tar" ] || return 1
  [ -f "$root/oldstyle.tar" ] || return 1
  [ -f "$root/oldstyle-cluster.tar" ] || return 1
  [ -f "$root/gnu-oldstyle-cluster.tar" ] || return 1
  plain="$(tar.gnu -tf "$root/archive.tar")"
  gnu_plain="$(tar.gnu -tf "$root/gnu-attached.tar")"
  old="$(tar.gnu -tzf "$root/oldstyle.tar")"
  old_cluster="$(tar.gnu -tzf "$root/oldstyle-cluster.tar")"
  gnu_old_cluster="$(tar.gnu -tzf "$root/gnu-oldstyle-cluster.tar")"
  [ "$plain" = "$gnu_plain" ] && [ "$plain" = payload ] || return 1
  [ "$(tar.gnu -tf "$root/after-c.tar")" = "$(tar.gnu -tf "$root/gnu-after-c.tar")" ] || return 1
  [ "$(tar.gnu -tf "$root/after-c.tar")" = payload ] || return 1
  [ "$old" = payload ] || return 1
  [ "$old_cluster" = "$gnu_old_cluster" ] && [ "$old_cluster" = payload ] || return 1
  bash "$shim" -xf "$root/archive.tar" -C "$root/out" || return 1
  mkdir -p "$root/out-x" "$root/out-gnu-x"
  bash "$shim" x -f"$root/archive.tar" -C"$root/out-x" || return 1
  tar.gnu x -f"$root/archive.tar" -C"$root/out-gnu-x" || return 1
  cmp -s "$root/cache/payload" "$root/out/payload" || return 1
  cmp -s "$root/cache/payload" "$root/out-x/payload" || return 1
  cmp -s "$root/out-x/payload" "$root/out-gnu-x/payload"
}

case_legacy_unknown_after_value_flags() {
  local root="$work/legacy-unknown" shim_f_status=0 gnu_f_status=0
  local shim_c_status=0 gnu_c_status=0
  rm -rf -- "$root"
  mkdir -p "$root/cache"
  printf 'legacy-ok\n' >"$root/cache/payload"
  (
    cd "$root" || return 1
    bash "$shim" cfq shim-f.tar -C cache payload >shim-f.out 2>&1
  ) || shim_f_status=$?
  (
    cd "$root" || return 1
    tar.gnu cfq gnu-f.tar -C cache payload >gnu-f.out 2>&1
  ) || gnu_f_status=$?
  (
    cd "$root" || return 1
    bash "$shim" cCfy cache shim-c.tar payload >shim-c.out 2>&1
  ) || shim_c_status=$?
  (
    cd "$root" || return 1
    tar.gnu cCfy cache gnu-c.tar payload >gnu-c.out 2>&1
  ) || gnu_c_status=$?
  [ "$shim_f_status" -ne 0 ] && [ "$gnu_f_status" -ne 0 ] || return 1
  [ "$shim_c_status" -ne 0 ] && [ "$gnu_c_status" -ne 0 ] || return 1
  [ ! -e "$root/q" ] && [ ! -e "$root/y" ] || return 1
  [ ! -e "$root/shim-f.tar" ] && [ ! -e "$root/shim-c.tar" ]
}

# Fail if a compressed -P extract writes an uncompressed archive under TMPDIR.
case_stream_no_raw() {
  local root="$work/stream"
  local payload limit real_mktemp
  rm -rf -- "$root"
  mkdir -p "$root/src" "$root/bin" "$root/dest/work/a/b/c"
  payload="$(perl -e 'print "Q" x 100000')"
  printf K >"$root/src/keep.txt"
  write_ustar "$root/arc.tar" \
    f keep.txt K \
    f ../../../.cache/a "$payload" \
    f ../../../.cache/b "$payload" || return 1
  gzip -c "$root/arc.tar" >"$root/arc.tar.gz" || return 1
  limit="$(stat -c '%s' "$root/arc.tar")"
  real_mktemp="$(command -v mktemp)"
  cat >"$root/bin/mktemp" <<EOF
#!/bin/bash
out="\$("$real_mktemp" "\$@")"
watch="\${VELNOR_TAR_WATCH:-}"
limit="\${VELNOR_TAR_LIMIT:-0}"
if [ -n "\$watch" ]; then
  for arg in "\$@"; do
    case "\$arg" in
      *velnor-tar-raw*) printf 'raw %s\\n' "\$out" >>"\$watch" ;;
    esac
  done
  if [ "\$limit" -gt 0 ]; then
    (
      while [ -e "\$out" ]; do
        if [ -f "\$out" ]; then
          sz="\$(stat -c '%s' "\$out" 2>/dev/null || echo 0)"
          if [ "\$sz" -ge "\$limit" ]; then
            printf 'big %s %s\\n' "\$sz" "\$out" >>"\$watch"
            exit 0
          fi
        fi
        sleep 0.02
      done
    ) >/dev/null 2>&1 &
  fi
fi
printf '%s\\n' "\$out"
EOF
  chmod 0755 "$root/bin/mktemp"
  stream_extract "$root/stage" "$root" "$limit" -xzf "$root/arc.tar.gz" -P -C "$root/dest/work/a/b/c" || return 1
  [ "$(cat "$root/dest/work/a/b/c/keep.txt")" = K ] || return 1
  [ "$(cat "$root/dest/work/.cache/a")" = "$payload" ] || return 1
  [ "$(cat "$root/dest/work/.cache/b")" = "$payload" ] || return 1
  rm -rf -- "$root/dest"
  mkdir -p "$root/dest/work/a/b/c"
  stream_extract "$root/stage2" "$root" "$limit" -xf "$root/arc.tar.gz" -P --use-compress-program "gzip -dc" -C "$root/dest/work/a/b/c" || return 1
  [ "$(cat "$root/dest/work/a/b/c/keep.txt")" = K ] || return 1
  [ "$(cat "$root/dest/work/.cache/a")" = "$payload" ] || return 1
  [ "$(cat "$root/dest/work/.cache/b")" = "$payload" ] || return 1
}

run_case collision-a-then-b case_collision_ab
run_case collision-b-then-a case_collision_ba
run_case filter-safe-member case_filter_one
run_case repeated-path-last-wins case_repeat
run_case flag-no-same-owner case_no_same_owner
run_case flag-delay-directory-restore case_delay_dir
run_case flag-bzip2 case_bzip2
run_case reject-parent-without-p case_escape
run_case verbose-identical case_verbose_same
run_case safe-hardlink-with-unsafe case_safe_hardlink
run_case unsafe-hardlink-fails case_unsafe_hardlink
run_case plain-extract case_plain
run_case absolute-file case_absolute_file
run_case legacy-unknown-after-value-flags case_legacy_unknown_after_value_flags

# List twice, then the extract decompress fails. The producer becomes a
# zombie while BusyBox is blocked on the fifo. That must fail, not hang.
case_emit_death_does_not_hang() {
  local root="$work/emit-die" pid i status=0
  rm -rf -- "$root"
  mkdir -p "$root/dest" "$root/bin"
  write_ustar "$root/arc.tar" \
    f keep.txt K \
    f ../../../.cache/a payload || return 1
  gzip -c "$root/arc.tar" >"$root/arc.tar.gz" || return 1
  cat >"$root/bin/flaky-dc" <<'EOF'
#!/bin/bash
nfile="${VELNOR_DC_COUNT:?}"
n=0
if [ -f "$nfile" ]; then
  n="$(cat "$nfile")"
fi
n=$((n + 1))
printf '%s' "$n" >"$nfile"
if [ "$n" -ge 3 ]; then
  exit 1
fi
exec gzip -dc
EOF
  chmod 0755 "$root/bin/flaky-dc"
  : >"$root/count"
  VELNOR_DC_COUNT="$root/count" \
    bash "$shim" -xf "$root/arc.tar.gz" -P \
    --use-compress-program "$root/bin/flaky-dc" -C "$root/dest" &
  pid=$!
  for i in 1 2 3 4 5 6 7 8; do
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid" || status=$?
      [ "$status" -ne 0 ]
      return
    fi
    sleep 0.5
  done
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  return 1
}

# actions/cache create: --posix -cf --exclude -P -C --files-from --use-compress-program.
# Hosted restore is GNU tar without --posix. Reject --posix on extract.
case_cache_posix() {
  local root="$work/posix" err status=0
  rm -rf -- "$root"
  mkdir -p "$root/src" "$root/dest"
  printf 'cache-ok\n' >"$root/src/g4-cache.txt"
  printf 'g4-cache.txt\n' >"$root/manifest"
  (
    cd "$root" || exit 1
    bash "$shim" --posix -cf cache.tzst --exclude cache.tzst -P \
      -C "$root/src" --files-from "$root/manifest" \
      --use-compress-program "gzip -c"
  ) || return 1
  tar.gnu -xf "$root/cache.tzst" -P -C "$root/dest" \
    --use-compress-program "gzip -dc" || return 1
  grep -qx cache-ok "$root/dest/g4-cache.txt" || return 1
  err="$root/err"
  run_shim "$err" --posix -xf "$root/cache.tzst" -P -C "$root/dest" || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q -- '--posix' "$err" || return 1
}

case_unicode() {
  local root="$work/unicode"
  rm -rf -- "$root"
  mkdir -p "$root/src" "$root/dest"
  printf 'uni-ok\n' >"$root/src/café.txt"
  printf '%s\n' 'café.txt' >"$root/manifest"
  bash "$shim" -cf "$root/arc.tar" -P -C "$root/src" --files-from "$root/manifest" || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest" || return 1
  grep -qx 'uni-ok' "$root/dest/café.txt" || return 1
}
