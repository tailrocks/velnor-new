# Planned extraction regressions shared by tar-semantics-test.sh.

case_strip_components_rejected_before_symlink_extract() {
  local root="$work/strip-components" err status=0
  rm -rf -- "$root"
  mkdir -p "$root/dest" "$root/outside"
  write_ustar "$root/arc.tar" s prefix/link ../outside || return 1
  err="$root/err"
  bash "$shim" -xf "$root/arc.tar" -C "$root/dest" --strip-components=1 >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q -- 'unsupported option --strip-components' "$err" || return 1
  [ ! -e "$root/dest/link" ] && [ ! -L "$root/dest/link" ] || return 1
  [ -z "$(find "$root/dest" -mindepth 1 -print -quit)" ] || return 1

  status=0
  bash "$shim" -xf "$root/arc.tar" -C "$root/dest" --strip-components 1 >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q -- 'unsupported option --strip-components' "$err" || return 1
  [ -z "$(find "$root/dest" -mindepth 1 -print -quit)" ] || return 1
}

case_external_relative_symlink() {
  local root="$work/external-link" status=0
  rm -rf -- "$root"
  mkdir -p "$root/dest/workspace/repo" "$root/dest/workspace/external/cache"
  write_ustar "$root/link.tar" s ../external/cache/link ../../shared/cache-target || return 1
  bash "$shim" -xf "$root/link.tar" -P -C "$root/dest/workspace/repo" || return 1
  [ -L "$root/dest/workspace/external/cache/link" ] || return 1
  [ "$(readlink "$root/dest/workspace/external/cache/link")" = ../../shared/cache-target ] || return 1
  rm -f -- "$root/dest/workspace/external/cache/link"

  write_ustar "$root/traversal.tar" \
    s ../external/cache/link ../../shared \
    f ../external/cache/link/pwned payload || return 1
  bash "$shim" -xf "$root/traversal.tar" -P -C "$root/dest/workspace/repo" >"$root/err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q 'traverses symlink' "$root/err" || return 1
  [ ! -e "$root/dest/workspace/external/cache/link" ] || return 1
  [ ! -e "$root/dest/workspace/shared/pwned" ] || return 1
}

case_directory_metadata() {
  local root="$work/directory-meta" existing fresh mode_existing mode_fresh time_existing time_fresh
  rm -rf -- "$root"
  existing="$root/dest/workspace/external/existing/cache"
  fresh="$root/dest/workspace/external/fresh/cache"
  mkdir -p "$root/dest/workspace/repo" "$existing"
  chmod 0755 "$existing"
  touch -d @1111111111 "$existing"
  write_ustar "$root/arc.tar" \
    d ../external/existing/cache/ 0700 1234567890 \
    f ../external/existing/cache/child existing \
    d ../external/fresh/cache/ 0710 1234567890 \
    f ../external/fresh/cache/child fresh || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest/workspace/repo" || return 1
  mode_existing="$(stat -c %a "$existing")"
  mode_fresh="$(stat -c %a "$fresh")"
  time_existing="$(stat -c %Y "$existing")"
  time_fresh="$(stat -c %Y "$fresh")"
  [ "$mode_existing" = 700 ] && [ "$time_existing" = 1234567890 ] || return 1
  [ "$mode_fresh" = 710 ] && [ "$time_fresh" = 1234567890 ] || return 1
  grep -qx existing "$existing/child" || return 1
  grep -qx fresh "$fresh/child" || return 1
}

case_restrictive_parent_metadata() {
  local root="$work/restrictive-parent" parent child existing path mode mtime
  rm -rf -- "$root"
  parent="../external/parent/"
  child="../external/parent/child/"
  existing="../external/existing/"
  mkdir -p "$root/dest/workspace/repo" "$root/gnu/workspace/repo" \
    "$root/dest/workspace/external/existing" "$root/gnu/workspace/external/existing" \
    "$root/stage"
  chmod 0750 "$root/dest/workspace/external/existing" \
    "$root/gnu/workspace/external/existing"
  touch -d @1111111111 "$root/dest/workspace/external/existing" \
    "$root/gnu/workspace/external/existing"
  write_ustar "$root/arc.tar" \
    d "$parent" 0500 1234567890 \
    d "$child" 0300 1234567891 \
    f ../external/parent/child/payload preserved \
    d "$existing" 0500 1234567880 \
    f ../external/existing/payload existing || return 1
  chmod 0755 "$work" "$rundir"
  chown -R nobody:nogroup "$root"
  runuser -u nobody -- tar.gnu -xf "$root/arc.tar" -P \
    -C "$root/gnu/workspace/repo" || {
      printf 'GNU restrictive-parent extract failed\n' >&2
      return 1
    }
  runuser -u nobody -- env TMPDIR="$root/stage" \
    bash "$shim" -xf "$root/arc.tar" -P \
      -C "$root/dest/workspace/repo" || {
        printf 'shim restrictive-parent extract failed\n' >&2
        return 1
      }
  for path in \
    "$root/gnu/workspace/external/parent" \
    "$root/dest/workspace/external/parent"; do
    mode="$(stat -c %a "$path")"
    mtime="$(stat -c %Y "$path")"
    [ "$mode" = 500 ] && [ "$mtime" = 1234567890 ] || {
      printf 'parent metadata mismatch path=%s mode=%s mtime=%s\n' "$path" "$mode" "$mtime" >&2
      return 1
    }
    grep -qx preserved "$path/child/payload" || {
      printf 'parent child payload missing path=%s\n' "$path" >&2
      return 1
    }
  done
  for path in \
    "$root/gnu/workspace/external/parent/child" \
    "$root/dest/workspace/external/parent/child"; do
    mode="$(stat -c %a "$path")"
    mtime="$(stat -c %Y "$path")"
    [ "$mode" = 300 ] && [ "$mtime" = 1234567891 ] || {
      printf 'child directory metadata mismatch path=%s mode=%s mtime=%s\n' "$path" "$mode" "$mtime" >&2
      return 1
    }
  done
  for path in \
    "$root/gnu/workspace/external/existing" \
    "$root/dest/workspace/external/existing"; do
    mode="$(stat -c %a "$path")"
    mtime="$(stat -c %Y "$path")"
    [ "$mode" = 500 ] && [ "$mtime" = 1234567880 ] || {
      printf 'existing directory metadata mismatch path=%s mode=%s mtime=%s\n' "$path" "$mode" "$mtime" >&2
      return 1
    }
    grep -qx existing "$path/payload" || {
      printf 'existing directory payload missing path=%s\n' "$path" >&2
      return 1
    }
  done
}

case_rewritten_long_paths() {
  local root="$work/rewritten-long" component archive i
  local prefix_source prefix_archive pax_source pax_archive
  rm -rf -- "$root"
  mkdir -p "$root"
  component="$(printf '%080d' 0 | tr '0' 'x')"

  prefix_source="$root/prefix/$component/$component/payload"
  prefix_archive="$root/prefix.tar"
  mkdir -p "${prefix_source%/*}"
  printf 'prefix-path\n' >"$prefix_source"
  (cd / && tar.gnu -P --format=pax -cf "$prefix_archive" "$prefix_source") || return 1
  rm -rf -- "$root/prefix"
  bash "$shim" -xf "$prefix_archive" -P -C "$root/dest" || return 1
  grep -qx prefix-path "$prefix_source" || return 1

  pax_source="$root/pax"
  for i in 1 2 3 4; do
    pax_source="$pax_source/$component"
  done
  pax_source="$pax_source/payload"
  pax_archive="$root/pax.tar"
  mkdir -p "${pax_source%/*}"
  printf 'pax-path\n' >"$pax_source"
  (cd / && tar.gnu -P --format=pax -cf "$pax_archive" "$pax_source") || return 1
  rm -rf -- "$root/pax"
  bash "$shim" -xf "$pax_archive" -P -C "$root/dest" || return 1
  grep -qx pax-path "$pax_source" || return 1
}

case_rewritten_pax_member() {
  local root="$work/rewritten-pax" target
  target="$root/dest/workspace/external/cache/pax-member"
  rm -rf -- "$root"
  mkdir -p "$root/dest/workspace/repo"
  write_ustar "$root/arc.tar" \
    x PaxHeaders/member ../external/cache/pax-member \
    f ignored pax-data || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest/workspace/repo" || return 1
  grep -qx pax-data "$target" || return 1
  [ ! -e "$root/dest/workspace/repo/external/cache/pax-member" ] || return 1
}

case_grouped_external_restore() {
  local root="$work/grouped" real_busybox file calls moves i count started_ns finished_ns elapsed_ms
  local -a records=()
  count="${VELNOR_PERF_COUNT:-100}"
  case "$count" in
    "" | *[!0-9]*) return 1 ;;
  esac
  [ "$count" -gt 0 ] || return 1
  rm -rf -- "$root"
  mkdir -p "$root/dest/workspace/repo" "$root/stage" "$root/bin"
  for i in $(seq 1 "$count"); do
    file="$(printf 'file-%05d' "$i")"
    records+=(f "../external/cache/$file" "payload-$i")
  done
  write_ustar "$root/arc.tar" "${records[@]}" || return 1
  real_busybox="$(command -v busybox)"
  cat >"$root/bin/busybox" <<'EOF'
#!/bin/sh
if [ "${1:-}" = tar ]; then
  printf x >>"$VELNOR_BUSYBOX_LOG"
fi
exec "$VELNOR_REAL_BUSYBOX" "$@"
EOF
  cat >"$root/bin/mv" <<'EOF'
#!/bin/sh
printf x >>"$VELNOR_MV_LOG"
exec /bin/mv "$@"
EOF
  chmod 0755 "$root/bin/busybox"
  chmod 0755 "$root/bin/mv"
  : >"$root/mv.log"
  started_ns="$(date +%s%N)"
  PATH="$root/bin:$PATH" \
    VELNOR_BUSYBOX_LOG="$root/busybox.log" \
    VELNOR_REAL_BUSYBOX="$real_busybox" \
    VELNOR_MV_LOG="$root/mv.log" \
    TMPDIR="$root/stage" \
    bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest/workspace/repo" || return 1
  finished_ns="$(date +%s%N)"
  elapsed_ms=$(((finished_ns - started_ns) / 1000000))
  calls="$(wc -c <"$root/busybox.log" | tr -d ' ')"
  moves="$(wc -c <"$root/mv.log" | tr -d ' ')"
  printf 'MEASURE external-restore-%s elapsed-ms=%s busybox-tar-calls=%s member-moves=%s\n' \
    "$count" "$elapsed_ms" "$calls" "$moves"
  [ "$calls" = 1 ] && [ "$moves" = 0 ] || return 1
  for i in 1 "$((count / 2))" "$count"; do
    file="$(printf 'file-%05d' "$i")"
    grep -qx "payload-$i" "$root/dest/workspace/external/cache/$file" || return 1
  done
  [ "$(find "$root/dest/workspace/external/cache" -type f | wc -l | tr -d ' ')" = "$count" ] || return 1
  [ -z "$(find "$root/stage" -mindepth 1 -print -quit)" ] || return 1
}
