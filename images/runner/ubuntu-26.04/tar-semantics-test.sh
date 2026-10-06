#!/bin/bash
# Archive cases the collision suite does not cover. Runs the worktree shim.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"

if ! command -v busybox >/dev/null 2>&1 || ! command -v tar.gnu >/dev/null 2>&1 || \
  ! command -v perl >/dev/null 2>&1 || ! command -v runuser >/dev/null 2>&1 || \
  [ "$(id -u)" -ne 0 ] || [ ! -d /lowdisk ]; then
  exec docker run --rm \
    -e DEBIAN_FRONTEND=noninteractive \
    -e VELNOR_PERF_COUNT \
    -e VELNOR_ONLY_GROUPED \
    --tmpfs /lowdisk:rw,mode=1777,size=1048576 \
    -v "$repo:/src:ro" \
    -w /src \
    debian:bookworm-slim \
    bash -lc 'apt-get update -qq && apt-get install -y -qq busybox tar perl gzip util-linux >/dev/null && ln -sfn "$(command -v tar)" /usr/bin/tar.gnu && exec bash images/runner/ubuntu-26.04/tar-semantics-test.sh'
fi

rundir="$(mktemp -d /tmp/tar-sem-layout.XXXXXX)"
cp "$here/tar-shim.sh" "$here/tar-absolute.sh" "$here/tar-extract.sh" "$here/tar-extract-plan.sh" "$rundir/"
cp "$here/tar-pax.pl" "$rundir/velnor-tar-pax"
cp "$here/tar-member.pl" "$rundir/"
cp "$here/tar-dir-meta.pl" "$rundir/"
cp "$here/tar-member-stream.pl" "$rundir/"
cp "$here/tar-member-rewrite.pl" "$rundir/"
chmod 0755 "$rundir/tar-shim.sh" "$rundir/velnor-tar-pax" "$rundir/tar-member.pl"
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

work="$(mktemp -d /tmp/tar-sem.XXXXXX)"
trap 'rm -rf -- "$work" "$rundir"' EXIT

write_ustar() {
  perl - "$@" <<'END_PERL'
use strict;
use warnings;
use bytes;
my $out = shift @ARGV;
open my $fh, ">:raw", $out or die $!;
select $fh;
sub header {
    my ($name, $type, $size, $link, $mode, $mtime) = @_;
    $link //= "";
    $mode //= 0644;
    $mtime //= 0;
    my $buf = "\0" x 512;
    die "name too long\n" if length($name) > 100;
    substr($buf, 0, length($name)) = $name;
    substr($buf, 100, 7) = sprintf("%07o", $mode);
    substr($buf, 108, 7) = sprintf("%07o", 0);
    substr($buf, 116, 7) = sprintf("%07o", 0);
    substr($buf, 124, 11) = sprintf("%011o", $size);
    substr($buf, 136, 11) = sprintf("%011o", $mtime);
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
    } elsif ($kind eq "s") {
        my $name = shift @ARGV;
        my $target = shift @ARGV;
        header($name, "2", 0, $target);
    } elsif ($kind eq "d") {
        my $name = shift @ARGV;
        my $mode = oct(shift @ARGV);
        my $mtime = shift @ARGV;
        header($name, "5", 0, "", $mode, $mtime);
    } elsif ($kind eq "x") {
        my $name = shift @ARGV;
        my $path = shift @ARGV;
        my $body = " path=$path\n";
        my $length = length($body) + 1;
        my $record;
        while (1) {
            $record = "$length$body";
            last if length($record) == $length;
            $length = length($record);
        }
        header($name, "x", length($record), "");
        print $record or die $!;
        my $pad = (512 - (length($record) % 512)) % 512;
        print "\0" x $pad if $pad;
    } else {
        die "bad record $kind\n";
    }
}
print "\0" x 1024 or die $!;
END_PERL
}

. "$here/tar-semantics-planned-test.sh"

case_newline() {
  local root="$work/newline" err status=0
  rm -rf -- "$root"
  mkdir -p "$root/dest"
  write_ustar "$root/arc.tar" f $'bad\nname' EVIL || return 1
  err="$root/err"
  bash "$shim" -xf "$root/arc.tar" -C "$root/dest" >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q 'newline' "$err" || return 1
  [ ! -e "$root/dest/bad" ] || return 1
}

case_symlink_escape() {
  local root="$work/escape" err status=0
  rm -rf -- "$root"
  mkdir -p "$root/dest" "$root/outside"
  write_ustar "$root/arc.tar" s link ../outside f link/pwned PWNED || return 1
  err="$root/err"
  bash "$shim" -xf "$root/arc.tar" -C "$root/dest" >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -E -q 'escapes|traverses' "$err" || return 1
  [ ! -e "$root/outside/pwned" ] || return 1
  [ ! -e "$root/dest/pwned" ] || return 1
}

case_preexisting_symlink() {
  local root="$work/pre" err status=0
  rm -rf -- "$root"
  mkdir -p "$root/dest" "$root/outside"
  ln -s "$root/outside" "$root/dest/link"
  write_ustar "$root/arc.tar" f link/pwned PWNED || return 1
  err="$root/err"
  bash "$shim" -xf "$root/arc.tar" -C "$root/dest" >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q 'traverses' "$err" || return 1
  [ ! -e "$root/outside/pwned" ] || return 1
}

case_symlink_inside() {
  local root="$work/inside"
  rm -rf -- "$root"
  mkdir -p "$root/src" "$root/dest"
  printf 'target-ok\n' >"$root/src/target.txt"
  ln -s target.txt "$root/src/link"
  printf 'link\n' >"$root/manifest"
  bash "$shim" -cf "$root/arc.tar" -P -C "$root/src" --files-from "$root/manifest" || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest" || return 1
  [ "$(readlink "$root/dest/link")" = target.txt ] || return 1
}

case_mode_and_mtime() {
  local root="$work/meta" mode_tool mode_data mtime
  rm -rf -- "$root"
  mkdir -p "$root/src" "$root/dest"
  printf 'run\n' >"$root/src/tool"
  printf 'data\n' >"$root/src/data"
  chmod 0755 "$root/src/tool"
  chmod 0640 "$root/src/data"
  touch -d @1234567890 "$root/src/tool" "$root/src/data"
  printf 'tool\ndata\n' >"$root/manifest"
  bash "$shim" -cf "$root/arc.tar" -P -C "$root/src" --files-from "$root/manifest" || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest" || return 1
  mode_tool="$(stat -c %a "$root/dest/tool")"
  mode_data="$(stat -c %a "$root/dest/data")"
  mtime="$(stat -c %Y "$root/dest/tool")"
  [ "$mode_tool" = 755 ] || return 1
  [ "$mode_data" = 640 ] || return 1
  [ "$mtime" = 1234567890 ] || return 1
  [ -x "$root/dest/tool" ] || return 1
}

case_empty_dir() {
  local root="$work/empty"
  rm -rf -- "$root"
  mkdir -p "$root/src/empty" "$root/dest"
  printf 'empty\n' >"$root/manifest"
  bash "$shim" -cf "$root/arc.tar" -P -C "$root/src" --files-from "$root/manifest" || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest" || return 1
  [ -d "$root/dest/empty" ] || return 1
  [ -z "$(find "$root/dest/empty" -mindepth 1 -print -quit)" ] || return 1
}

case_long_and_deep() {
  local root="$work/long" long deep i
  rm -rf -- "$root"
  mkdir -p "$root/src" "$root/dest"
  long=""
  for i in $(seq 1 180); do
    long="${long}p"
  done
  long="${long}.txt"
  printf 'long-ok\n' >"$root/src/$long"
  deep="f.txt"
  for i in $(seq 1 30); do
    deep="dir/$deep"
  done
  mkdir -p "$root/src/$(dirname "$deep")"
  printf 'deep-ok\n' >"$root/src/$deep"
  printf '%s\n%s\n' "$long" "$deep" >"$root/manifest"
  bash "$shim" -cf "$root/arc.tar" -P -C "$root/src" --files-from "$root/manifest" || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest" || return 1
  perl "$rundir/tar-member.pl" --list <"$root/arc.tar" >"$root/list" || return 1
  grep -Fxq -- "$long" "$root/list" || return 1
  grep -Fxq -- "$deep" "$root/list" || return 1
  grep -qx 'long-ok' "$root/dest/$long" || return 1
  grep -qx 'deep-ok' "$root/dest/$deep" || return 1
}

case_space_name() {
  local root="$work/space"
  rm -rf -- "$root"
  mkdir -p "$root/src" "$root/dest"
  printf 'space-ok\n' >"$root/src/my file.txt"
  printf 'my file.txt\n' >"$root/manifest"
  bash "$shim" -cf "$root/arc.tar" -P -C "$root/src" --files-from "$root/manifest" || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest" || return 1
  grep -qx 'space-ok' "$root/dest/my file.txt" || return 1
}

case_absolute_relative_target() {
  local root="$work/absrel" link
  rm -rf -- "$root"
  mkdir -p "$root/dest" "$root/pit"
  link="$root/pit/link"
  write_ustar "$root/arc.tar" s "$link" tmp || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest" || return 1
  [ -L "$link" ] || return 1
  [ "$(readlink "$link")" = tmp ] || return 1
  [ ! -e "$root/pit/tmp" ] || return 1
}

case_absolute_link_target() {
  local root="$work/abslink" status=0
  rm -rf -- "$root"
  mkdir -p "$root/dest" "$root/outside"
  write_ustar "$root/arc.tar" s link "$root/outside" || return 1
  bash "$shim" -xf "$root/arc.tar" -P -C "$root/dest" >"$root/err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q 'symlink escapes destination' "$root/err" || return 1
  [ ! -e "$root/dest/link" ] && [ ! -L "$root/dest/link" ] || return 1
}

case_dot_symlink_walk() {
  local root="$work/dotwalk" err status=0
  rm -rf -- "$root"
  mkdir -p "$root/dest" "$root/outside"
  write_ustar "$root/arc.tar" s link nested f ./link/pwned PWNED || return 1
  err="$root/err"
  bash "$shim" -xf "$root/arc.tar" -C "$root/dest" >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q 'traverses' "$err" || return 1
  [ ! -e "$root/dest/nested/pwned" ] || return 1
  [ ! -e "$root/dest/pwned" ] || return 1
  [ ! -e "$root/outside/pwned" ] || return 1
}

case_corrupt_header() {
  local root="$work/corrupt" err status=0 pstatus=0
  rm -rf -- "$root"
  mkdir -p "$root/dest"
  write_ustar "$root/arc.tar" f note.txt hello || return 1
  perl -e 'open my $f, "+<:raw", shift or die $!; print $f "X"' "$root/arc.tar" || return 1
  err="$root/err"
  bash "$shim" -xf "$root/arc.tar" -C "$root/dest" >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -E -q 'bad checksum|does not look like a tar archive' "$err" || return 1
  [ ! -e "$root/dest/note.txt" ] || return 1
  perl "$rundir/tar-member.pl" --list <"$root/arc.tar" >"$root/list" 2>"$root/perlerr" || pstatus=$?
  [ "$pstatus" -ne 0 ] || return 1
  grep -F -q 'bad checksum' "$root/perlerr" || return 1
}

case_list_drains_tail() {
  local root="$work/drain" status=0
  rm -rf -- "$root"
  mkdir -p "$root"
  write_ustar "$root/arc.tar" f note.txt hello || return 1
  # The writer keeps the pipe open after the end marker. A reader that
  # stops there makes the later write SIGPIPE, and pipefail fails the list.
  set +e
  (
    set -o pipefail
    {
      cat "$root/arc.tar"
      sleep 0.2
      printf 'x'
    } | perl "$rundir/tar-member.pl" --list >"$root/list"
  )
  status=$?
  set -e
  [ "$status" -eq 0 ] || return 1
  grep -F -q 'note.txt' "$root/list" || return 1
}

case_partial_archive() {
  local root="$work/partial" err status=0 pstatus=0
  rm -rf -- "$root"
  mkdir -p "$root/dest"
  write_ustar "$root/arc.tar" f note.txt hello-partial-body || return 1
  dd if="$root/arc.tar" of="$root/cut.tar" bs=1 count=514 status=none || return 1
  err="$root/err"
  bash "$shim" -xf "$root/cut.tar" -C "$root/dest" >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -E -q 'short read|Unexpected EOF' "$err" || return 1
  [ ! -e "$root/dest/note.txt" ] || return 1
  perl "$rundir/tar-member.pl" --list <"$root/cut.tar" >"$root/list" 2>"$root/perlerr" || pstatus=$?
  [ "$pstatus" -ne 0 ] || return 1
  grep -F -q 'short read' "$root/perlerr" || return 1
}

case_cargo_work_symlink() {
  local repo err status=0
  repo=/home/runner/work/repo/repo
  rm -rf -- /home/runner/work
  mkdir -p "$repo"
  write_ustar "$work/rls.tar" s ../../_temp/velnor/cargo/bin/rls rustup || return 1
  bash "$shim" -xf "$work/rls.tar" -P -C "$repo" || return 1
  [ -L /home/runner/work/_temp/velnor/cargo/bin/rls ] || return 1
  [ "$(readlink /home/runner/work/_temp/velnor/cargo/bin/rls)" = rustup ] || return 1
  write_ustar "$work/bad.tar" s ../../_temp/velnor/cargo/bin/bad /etc/passwd || return 1
  err="$work/bad.err"
  bash "$shim" -xf "$work/bad.tar" -P -C "$repo" >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q 'escapes' "$err" || return 1
  [ ! -e /home/runner/work/_temp/velnor/cargo/bin/bad ] || return 1
  [ ! -L /etc/passwd ] || return 1
  status=0
  write_ustar "$work/out.tar" s ../../../../../etc/outside-link passwd || return 1
  bash "$shim" -xf "$work/out.tar" -P -C "$repo" >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  [ ! -e /etc/outside-link ] && [ ! -L /etc/outside-link ] || return 1
}

case_cargo_bulk_parents() {
  local repo i
  local -a args=()
  repo=/home/runner/work/repo/repo
  rm -rf -- /home/runner/work
  mkdir -p "$repo"
  args=("$work/bulk.tar")
  for ((i = 1; i <= 40; i++)); do
    args+=(f "../../_temp/velnor/cargo/reg/$i" "body-$i")
  done
  args+=(s ../../_temp/velnor/cargo/bin/rls rustup)
  write_ustar "${args[@]}" || return 1
  bash "$shim" -xf "$work/bulk.tar" -P -C "$repo" || return 1
  [ "$(readlink /home/runner/work/_temp/velnor/cargo/bin/rls)" = rustup ] || return 1
  for ((i = 1; i <= 40; i++)); do
    [ "$(cat "/home/runner/work/_temp/velnor/cargo/reg/$i")" = "body-$i" ] || return 1
  done
}

case_disk_full() {
  local root="$work/diskfull" err status=0 bytes
  [ -d /lowdisk ] || return 1
  rm -rf -- "$root" /lowdisk/dest
  mkdir -p "$root/src" /lowdisk/dest
  dd if=/dev/zero of="$root/src/big.bin" bs=1024 count=2048 status=none || return 1
  bash "$shim" -cf "$root/arc.tar" -C "$root/src" big.bin || return 1
  err="$root/err"
  bash "$shim" -xf "$root/arc.tar" -C /lowdisk/dest >"$err" 2>&1 || status=$?
  [ "$status" -ne 0 ] || return 1
  grep -F -q 'No space left on device' "$err" || return 1
  if [ -f /lowdisk/dest/big.bin ]; then
    bytes="$(wc -c </lowdisk/dest/big.bin)"
    [ "$bytes" -lt 2097152 ] || return 1
  fi
}

if [ "${VELNOR_ONLY_GROUPED:-0}" = 1 ]; then
  run_case grouped-external-restore case_grouped_external_restore
  printf 'RESULT pass=%s fail=%s\n' "$pass" "$fail"
  [ "$fail" -eq 0 ]
  exit
fi

run_case newline-rejected case_newline
run_case absolute-relative-target-preserved case_absolute_relative_target
run_case absolute-link-target-rejected case_absolute_link_target
run_case dot-symlink-walk case_dot_symlink_walk
run_case symlink-escape case_symlink_escape
run_case strip-components-rejected-before-symlink-extract case_strip_components_rejected_before_symlink_extract
run_case preexisting-symlink case_preexisting_symlink
run_case symlink-inside case_symlink_inside
run_case external-relative-symlink case_external_relative_symlink
run_case cargo-work-symlink case_cargo_work_symlink
run_case cargo-bulk-parents case_cargo_bulk_parents
run_case mode-and-mtime case_mode_and_mtime
run_case directory-metadata case_directory_metadata
run_case many-directory-metadata case_many_directory_metadata
run_case record-failure-keeps-mode case_record_failure_keeps_mode
run_case short-restore-applies-complete-rows case_short_restore_applies_complete_rows
run_case record-failure-removes-batch case_record_failure_removes_batch
run_case restrictive-parent-metadata case_restrictive_parent_metadata
run_case rewritten-long-paths case_rewritten_long_paths
run_case rewritten-pax-member case_rewritten_pax_member
run_case grouped-external-restore case_grouped_external_restore
run_case grouped-extract-failure-keeps-prior-mode case_grouped_extract_failure_keeps_prior_mode
run_case empty-dir case_empty_dir
run_case long-and-deep case_long_and_deep
run_case gnu-enosys-still-extracts case_gnu_enosys_still_extracts
run_case space-name case_space_name
run_case corrupt-header case_corrupt_header
run_case list-drains-tail case_list_drains_tail
run_case partial-archive case_partial_archive
run_case disk-full case_disk_full

printf 'RESULT pass=%s fail=%s\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
