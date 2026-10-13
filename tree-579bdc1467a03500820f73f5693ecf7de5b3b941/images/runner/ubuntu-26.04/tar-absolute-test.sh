#!/bin/bash
# Regression for tar-shim extract. Runs the real shim, not a reimplementation.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"

if ! command -v busybox >/dev/null 2>&1 || ! command -v tar.gnu >/dev/null 2>&1 || ! command -v perl >/dev/null 2>&1; then
  exec docker run --rm \
    -e DEBIAN_FRONTEND=noninteractive \
    -v "$repo:/src" \
    -w /src \
    debian:bookworm-slim \
    bash -lc 'apt-get update -qq && apt-get install -y -qq busybox tar perl gzip >/dev/null && ln -sfn "$(command -v tar)" /usr/bin/tar.gnu && exec bash images/runner/ubuntu-26.04/tar-absolute-test.sh'
fi

# Image layout: shim and helpers share a directory, pax is velnor-tar-pax.
rundir="$(mktemp -d /tmp/tar-shim-layout.XXXXXX)"
cp "$here/tar-shim.sh" "$here/tar-absolute.sh" "$rundir/"
cp "$here/tar-pax.pl" "$rundir/velnor-tar-pax"
if [ -f "$here/tar-extract.sh" ]; then
  cp "$here/tar-extract.sh" "$rundir/"
fi
if [ -f "$here/tar-member.pl" ]; then
  cp "$here/tar-member.pl" "$rundir/"
fi
chmod 0755 "$rundir/tar-shim.sh" "$rundir/velnor-tar-pax"
shim="$rundir/tar-shim.sh"
pass=0
fail=0

run_case() {
  local name="$1"
  shift
  if "$@"; then
    pass=$((pass + 1))
    printf 'PASS %s\n' "$name"
  else
    fail=$((fail + 1))
    printf 'FAIL %s\n' "$name"
  fi
}

work="$(mktemp -d /tmp/tar-abs-test.XXXXXX)"
trap 'rm -rf -- "$work" "$rundir"' EXIT

# Minimal ustar. Args: out, then records "f name data" or "h name target".
write_ustar() {
  perl - "$@" <<'END_PERL'
use strict;
use warnings;
use bytes;
my $out = shift @ARGV;
open my $fh, ">:raw", $out or die $!;
select $fh;
sub cstr_field {
    my ($buf, $at, $val) = @_;
    return if $val eq "";
    die "field too long\n" if length($val) > 100;
    substr($_[0], $at, length($val)) = $val;
}
sub header {
    my ($name, $type, $size, $link) = @_;
    $link //= "";
    my $buf = "\0" x 512;
    die "name too long\n" if length($name) > 100;
    substr($buf, 0, length($name)) = $name;
    substr($buf, 100, 7) = sprintf("%07o", 0644);
    substr($buf, 108, 7) = sprintf("%07o", 0);
    substr($buf, 116, 7) = sprintf("%07o", 0);
    substr($buf, 124, 11) = sprintf("%011o", $size);
    substr($buf, 136, 11) = sprintf("%011o", 0);
    substr($buf, 148, 8) = "        ";
    substr($buf, 156, 1) = $type;
    die "link too long\n" if length($link) > 100;
    substr($buf, 157, length($link)) = $link if $link ne "";
    substr($buf, 257, 6) = "ustar\0";
    substr($buf, 263, 2) = "00";
    my $sum = 0;
    $sum += ord($_) for split //, $buf;
    substr($buf, 148, 8) = sprintf("%06o\0 ", $sum);
    print $buf or die $!;
}
while (@ARGV) {
    my $kind = shift @ARGV;
    if ($kind eq "f") {
        my $name = shift @ARGV;
        my $data = shift @ARGV;
        header($name, "0", length($data), "");
        print $data or die $!;
        my $pad = (512 - (length($data) % 512)) % 512;
        print "\0" x $pad if $pad;
    } elsif ($kind eq "h") {
        my $name = shift @ARGV;
        my $target = shift @ARGV;
        header($name, "1", 0, $target);
    } else {
        die "bad record $kind\n";
    }
}
print "\0" x 1024 or die $!;
END_PERL
}

run_shim() {
  local errfile="$1"
  shift
  local status=0
  bash "$shim" "$@" >"$errfile.out" 2>"$errfile" || status=$?
  cat "$errfile.out" >>"$errfile"
  rm -f -- "$errfile.out"
  return "$status"
}

case_collision() {
  local order="$1"
  local root="$work/col-$order"
  rm -rf -- "$root"
  local long
  long="$(printf 'n%.0s' $(seq 1 90))"
  mkdir -p "$root/src/work/a/b/c/.cache" "$root/src/work/.cache/mbx/$long/sub" "$root/dest/work/a/b/c" "$root/stage"
  printf A >"$root/src/work/a/b/c/.cache/x"
  printf B >"$root/src/work/.cache/x"
  printf L >"$root/src/work/.cache/mbx/$long/sub/file"
  printf K >"$root/src/work/a/b/c/keep.txt"
  if [ "$order" = ab ]; then
    printf '%s\n' .cache/x ../../../.cache/x "../../../.cache/mbx/$long/sub/file" keep.txt >"$root/manifest"
  else
    printf '%s\n' ../../../.cache/x .cache/x keep.txt >"$root/manifest"
  fi
  bash "$shim" -cf "$root/arc.tar" -P -C "$root/src/work/a/b/c" --files-from "$root/manifest" || return 1
  TMPDIR="$root/stage" bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest/work/a/b/c" || return 1
  [ "$(cat "$root/dest/work/a/b/c/.cache/x" 2>/dev/null || echo MISSING)" = A ] || {
    printf 'inside=%s outside=%s\n' "$(cat "$root/dest/work/a/b/c/.cache/x" 2>/dev/null || echo MISSING)" "$(cat "$root/dest/work/.cache/x" 2>/dev/null || echo MISSING)" >&2
    find "$root/dest" -type f -print >&2 || true
    return 1
  }
  [ "$(cat "$root/dest/work/.cache/x" 2>/dev/null || echo MISSING)" = B ] || return 1
  [ "$(cat "$root/dest/work/a/b/c/keep.txt" 2>/dev/null || echo MISSING)" = K ] || return 1
  if [ "$order" = ab ]; then
    [ "$(cat "$root/dest/work/.cache/mbx/$long/sub/file")" = L ] || return 1
    [ ! -e "$root/dest/work/a/b/c/.cache/mbx/$long/sub/file" ] || return 1
    gzip -c "$root/arc.tar" >"$root/arc.tar.gz" || return 1
    rm -rf -- "$root/gz"
    mkdir -p "$root/gz/work/a/b/c" "$root/gzstage"
    TMPDIR="$root/gzstage" bash "$shim" -xzf "$root/arc.tar.gz" -P -C "$root/gz/work/a/b/c" || return 1
    [ "$(cat "$root/gz/work/a/b/c/.cache/x")" = A ] || return 1
    [ "$(cat "$root/gz/work/.cache/x")" = B ] || return 1
    [ "$(cat "$root/gz/work/.cache/mbx/$long/sub/file")" = L ] || return 1
    [ -z "$(find "$root/gzstage" -mindepth 1 -print -quit)" ] || return 1
  fi
  [ -z "$(find "$root/stage" -mindepth 1 -print -quit)" ] || return 1
  local inodes
  inodes="$(find "$root/dest" -type f -printf '%i\n' | sort | uniq | wc -l | tr -d ' ')"
  local files
  files="$(find "$root/dest" -type f | wc -l | tr -d ' ')"
  [ "$inodes" = "$files" ] || return 1
}

case_collision_ab() { case_collision ab; }
case_collision_ba() { case_collision ba; }

case_filter_one() {
  local root="$work/filter"
  rm -rf -- "$root"
  mkdir -p "$root/src/work/a/b/c/.cache" "$root/src/work/.cache" "$root/dest/work/a/b/c"
  printf A >"$root/src/work/a/b/c/.cache/x"
  printf B >"$root/src/work/.cache/x"
  printf '%s\n' .cache/x ../../../.cache/x >"$root/manifest"
  bash "$shim" -cf "$root/arc.tar" -P -C "$root/src/work/a/b/c" --files-from "$root/manifest" || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest/work/a/b/c" .cache/x || return 1
  [ "$(cat "$root/dest/work/a/b/c/.cache/x")" = A ] || return 1
  [ ! -e "$root/dest/work/.cache/x" ] || return 1
}

case_repeat() {
  local root="$work/repeat"
  rm -rf -- "$root"
  mkdir -p "$root/dest/work/a/b/c" "$root/stage"
  write_ustar "$root/arc.tar" \
    f ../../../.cache/x first \
    f ../../../.cache/x second || return 1
  TMPDIR="$root/stage" bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest/work/a/b/c" || return 1
  [ "$(cat "$root/dest/work/.cache/x")" = second ] || return 1
  [ ! -e "$root/dest/work/a/b/c/.cache/x" ] || return 1
  [ -z "$(find "$root/stage" -mindepth 1 -print -quit)" ] || return 1
  [ "$(find "$root/dest" -type f | wc -l | tr -d ' ')" = 1 ] || return 1
}

case_flag() {
  local flag="$1"
  local root="$work/flag"
  mkdir -p "$root/src"
  printf x >"$root/src/f"
  local err="$root/err"
  local status=0
  run_shim "$err" "$flag" -cf "$root/out.tar" -C "$root/src" f || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q -- "$flag" "$err" || return 1
}

case_no_same_owner() { case_flag --no-same-owner; }
case_delay_dir() { case_flag --delay-directory-restore; }
case_bzip2() { case_flag -j; }

case_escape() {
  local root="$work/escape"
  rm -rf -- "$root"
  mkdir -p "$root/dest/a/b"
  printf OK >"$root/ok-bytes"
  write_ustar "$root/arc.tar" f ../escaped EVIL f ok.txt OK || return 1
  local err="$root/err"
  local status=0
  run_shim "$err" -xf "$root/arc.tar" -C "$root/dest/a/b" || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q 'escapes' "$err" || return 1
  [ ! -e "$root/dest/a/escaped" ] || return 1
  [ ! -e "$root/dest/a/b/escaped" ] || return 1
  [ ! -e "$root/dest/escaped" ] || return 1
  [ ! -e "$root/dest/a/b/ok.txt" ] || return 1
}

case_verbose_same() {
  local root="$work/verbose"
  rm -rf -- "$root"
  mkdir -p "$root/src" "$root/out1" "$root/out2"
  printf same >"$root/src/f"
  bash "$shim" -cf "$root/a.tar" -C "$root/src" f || return 1
  bash "$shim" -cvf "$root/b.tar" -C "$root/src" f || return 1
  cmp -s "$root/a.tar" "$root/b.tar" || return 1
  bash "$shim" -xf "$root/a.tar" -C "$root/out1" || return 1
  bash "$shim" -xvf "$root/a.tar" -C "$root/out2" || return 1
  cmp -s "$root/out1/f" "$root/out2/f" || return 1
}

case_safe_hardlink() {
  local root="$work/hlink"
  rm -rf -- "$root"
  mkdir -p "$root/dest/work/a/b/c"
  write_ustar "$root/arc.tar" \
    f a H \
    h b a \
    f ../../../.cache/x B || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest/work/a/b/c" || return 1
  [ "$(cat "$root/dest/work/a/b/c/a")" = H ] || return 1
  [ "$(cat "$root/dest/work/a/b/c/b")" = H ] || return 1
  [ "$(cat "$root/dest/work/.cache/x")" = B ] || return 1
  local ia ib
  ia="$(stat -c '%i' "$root/dest/work/a/b/c/a")"
  ib="$(stat -c '%i' "$root/dest/work/a/b/c/b")"
  [ "$ia" = "$ib" ] || return 1
}

case_unsafe_hardlink() {
  local root="$work/uhlink"
  rm -rf -- "$root"
  mkdir -p "$root/dest/work/a/b/c"
  write_ustar "$root/arc.tar" \
    f a H \
    h ../../../.cache/y a || return 1
  local err="$root/err"
  local status=0
  run_shim "$err" -xf "$root/arc.tar" -P -C "$root/dest/work/a/b/c" || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q 'hardlink' "$err" || return 1
  [ ! -e "$root/dest/work/.cache/y" ] || return 1
  [ ! -e "$root/dest/work/a/b/c/a" ] || return 1
}

case_plain() {
  local root="$work/plain"
  rm -rf -- "$root"
  mkdir -p "$root/src/sub" "$root/out" "$root/stripped"
  printf ok >"$root/src/sub/f"
  bash "$shim" -cf "$root/arc.tar" -C "$root/src" sub/f || return 1
  bash "$shim" -xf "$root/arc.tar" -C "$root/out" || return 1
  [ "$(cat "$root/out/sub/f")" = ok ] || return 1
  bash "$shim" -xf "$root/arc.tar" -C "$root/stripped" --strip-components=1 || return 1
  [ "$(cat "$root/stripped/f")" = ok ] || return 1
}

case_absolute_file() {
  local root="$work/absfile"
  rm -rf -- "$root"
  mkdir -p "$root/out"
  printf abs-ok >"$root/marker"
  bash "$shim" -cf "$root/arc.tar" -P "$root/marker" || return 1
  rm -f -- "$root/marker"
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/out" || return 1
  [ "$(cat "$root/marker")" = abs-ok ] || return 1
  [ ! -e "$root/out/marker" ] || return 1
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
  stream_extract() {
    local stage="$1"
    shift
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
  stream_extract "$root/stage" -xzf "$root/arc.tar.gz" -P -C "$root/dest/work/a/b/c" || return 1
  [ "$(cat "$root/dest/work/a/b/c/keep.txt")" = K ] || return 1
  [ "$(cat "$root/dest/work/.cache/a")" = "$payload" ] || return 1
  [ "$(cat "$root/dest/work/.cache/b")" = "$payload" ] || return 1
  rm -rf -- "$root/dest"
  mkdir -p "$root/dest/work/a/b/c"
  stream_extract "$root/stage2" -xf "$root/arc.tar.gz" -P --use-compress-program "gzip -dc" -C "$root/dest/work/a/b/c" || return 1
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

run_case dash-positional case_dash_positional
run_case stream-no-raw-archive case_stream_no_raw
run_case emit-death-does-not-hang case_emit_death_does_not_hang

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

run_case cache-posix case_cache_posix
run_case unicode-member case_unicode

printf 'RESULT pass=%s fail=%s\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
