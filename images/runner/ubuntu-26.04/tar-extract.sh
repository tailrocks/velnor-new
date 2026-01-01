# Sourced by tar-absolute.sh. -P members are planned into conflict-free
# extraction groups with rewritten relative names. Hardlinks need their target
# in the same BusyBox extract; staged merge groups preserve that inode.
# The decompressor is a stream. One fifo at a time reaches BusyBox, then it
# is removed. /tmp never holds the uncompressed archive.

stage_dir=""
seq_dir=""
plan_file=""
map_file=""
prod_pid=""
bb_pid=""
dest_root=""
isolated_any=0
extract_all=0
member_pl=""
dir_meta_file=""
dir_metadata_restored=0

cleanup_extract() {
  local exit_status=$?
  if [ -n "${bb_pid}" ]; then
    kill "${bb_pid}" 2>/dev/null || true
    wait "${bb_pid}" 2>/dev/null || true
    bb_pid=""
  fi
  if [ -n "${seq_dir}" ]; then
    : >"${seq_dir}/done" 2>/dev/null || true
  fi
  if [ -n "${prod_pid}" ]; then
    local child kids
    kids="$(ps -o pid= --ppid "${prod_pid}" 2>/dev/null || true)"
    for child in $kids; do
      kill "${child}" 2>/dev/null || true
    done
    kill "${prod_pid}" 2>/dev/null || true
    wait "${prod_pid}" 2>/dev/null || true
    prod_pid=""
  fi
  if [ "${dir_metadata_restored}" -eq 0 ] && \
    [ -n "${dir_meta_file}" ] && [ -s "${dir_meta_file}" ]; then
    restore_directory_metadata || true
  fi
  if [ -n "${dir_meta_file}" ]; then
    rm -f -- "$dir_meta_file"
    dir_meta_file=""
  fi
  if [ -n "${stage_dir}" ]; then
    rm -rf -- "$stage_dir"
    stage_dir=""
  fi
  if [ -n "${seq_dir}" ]; then
    rm -rf -- "$seq_dir"
    seq_dir=""
  fi
  if [ -n "${plan_file}" ]; then
    rm -f -- "$plan_file"
    plan_file=""
  fi
  if [ -n "${map_file}" ]; then
    rm -f -- "$map_file"
    map_file=""
  fi
  return "$exit_status"
}

path_under() {
  local root="$1"
  local path="$2"
  if [ "$root" = "/" ]; then
    return 0
  fi
  if [ "$path" = "$root" ] || [[ "$path" == "$root"/* ]]; then
    return 0
  fi
  return 1
}

# Without -P, absolute names and ".." segments are untrusted input.
member_rejected() {
  local name="$1"
  local base root intended
  case "$name" in
    /* | .. | ../* | */.. | */../*) return 0 ;;
  esac
  base="${chdir:-$PWD}"
  root="$(norm_path "$base")"
  intended="$(norm_path "$base/$name")"
  if path_under "$root" "$intended"; then
    return 1
  fi
  return 0
}

reject_untrusted() {
  local list member
  list="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-names.XXXXXX")"
  if ! list_members "$list"; then
    rm -f -- "$list"
    die "member list failed"
  fi
  while IFS= read -r member || [ -n "$member" ]; do
    [ -n "$member" ] || continue
    if member_rejected "$member"; then
      rm -f -- "$list"
      die "member escapes destination: $member"
    fi
  done <"$list"
  rm -f -- "$list"
}

member_tool() {
  member_pl="${_velnor_tar_here}/tar-member.pl"
  [ -f "$member_pl" ] || die "absolute names need $member_pl"
}

read_perl_members() {
  local list index type link name mode
  list="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-list.XXXXXX")"
  if ! stream_archive | perl "$member_pl" --list >"$list"; then
    rm -f -- "$list"
    die "member list failed"
  fi
  while IFS= read -r index && IFS= read -r type && IFS= read -r link && \
    IFS= read -r name && IFS= read -r mode; do
    [ "$index" = "${#mem_name[@]}" ] || die "member index gap"
    mem_name+=("$name")
    mem_type+=("$type")
    mem_link+=("$link")
    mem_mode+=("$mode")
  done <"$list"
  rm -f -- "$list"
}

read_gnu_names() {
  local list line
  gnu_names=()
  list="$(mktemp "${TMPDIR:-/tmp}/velnor-tar-gnu.XXXXXX")"
  # Same listing as list_members: perl stored names, never BusyBox, never GNU tar.
  if ! list_members "$list"; then
    rm -f -- "$list"
    die "member list failed"
  fi
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || continue
    gnu_names+=("$line")
  done <"$list"
  rm -f -- "$list"
}

load_members() {
  local -A want=()
  local -A seen=()
  local path i
  mem_name=()
  mem_type=()
  mem_link=()
  mem_mode=()
  mem_wanted=()
  mem_isolated=()
  mem_strip=()
  mem_emit_path=()
  extract_all=0
  if [ -z "$files_from" ] && [ "${#filtered[@]}" -eq 0 ]; then
    extract_all=1
  fi
  if [ "$extract_all" -eq 0 ]; then
    for path in "${filtered[@]}"; do
      want["$path"]=1
    done
  fi
  read_perl_members
  read_gnu_names
  if [ "${#mem_name[@]}" -ne "${#gnu_names[@]}" ]; then
    die "member list mismatch count gnu=${#gnu_names[@]} parsed=${#mem_name[@]}"
  fi
  for i in "${!mem_name[@]}"; do
    [ "${mem_name[$i]}" = "${gnu_names[$i]}" ] || die "member list mismatch at $i"
    case "${mem_name[$i]}" in
      *$'\t'*) die "member name contains a tab" ;;
      *$'\n'*) die "member name contains a newline" ;;
    esac
    if [ "$extract_all" -eq 1 ] || [ -n "${want[${mem_name[$i]}]:-}" ]; then
      mem_wanted[$i]=1
      seen["${mem_name[$i]}"]=1
    else
      mem_wanted[$i]=0
    fi
  done
  if [ "$extract_all" -eq 0 ]; then
    for path in "${filtered[@]}"; do
      [ -n "${seen[$path]:-}" ] || die "not found in archive: $path"
    done
  fi
  mark_isolated
  reject_symlink_traversal
}

mark_isolated() {
  local -A count=()
  local i stripped
  isolated_any=0
  if [ "${#mem_name[@]}" -eq 0 ]; then
    return 0
  fi
  for i in "${!mem_name[@]}"; do
    strip_unsafe "${mem_name[$i]}" stripped
    mem_strip[$i]="$stripped"
    count["$stripped"]=$((${count["$stripped"]:-0} + 1))
  done
  for i in "${!mem_name[@]}"; do
    stripped="${mem_strip[$i]}"
    if [ "$stripped" != "${mem_name[$i]}" ] || [ "${count[$stripped]}" -gt 1 ]; then
      mem_isolated[$i]=1
      isolated_any=1
    else
      mem_isolated[$i]=0
    fi
  done
}

# BusyBox drops a hardlink when its target is not in the same extract.
reject_hardlinks() {
  local -A seen=()
  local i run=-1 open=0 key
  if [ "${#mem_name[@]}" -eq 0 ]; then
    return 0
  fi
  for i in "${!mem_name[@]}"; do
    if [ "${mem_wanted[$i]}" -ne 1 ] || [ "${mem_isolated[$i]}" -eq 1 ]; then
      open=0
      if [ "${mem_wanted[$i]}" -eq 1 ] && [ "${mem_type[$i]}" = 1 ]; then
        die "hardlink in per-member extract: ${mem_name[$i]}"
      fi
      continue
    fi
    if [ "$open" -eq 0 ]; then
      run=$((run + 1))
      open=1
    fi
    if [ "${mem_type[$i]}" = 1 ]; then
      key="${run}"$'\n'"${mem_link[$i]}"
      [ -n "${seen[$key]:-}" ] || die "hardlink in per-member extract: ${mem_name[$i]}"
    fi
    key="${run}"$'\n'"${mem_name[$i]}"
    seen["$key"]=1
  done
}

# kill -0 is true for a zombie, so the state byte is what ends the poll.
child_state() {
  local pid="$1"
  local rest
  [ -r "/proc/$pid/stat" ] || return 1
  rest="$(sed -n 's/.*) //p' "/proc/$pid/stat")"
  printf '%s' "${rest%% *}"
}

# Producer death while BusyBox is blocked on a fifo is a hang, not a slow extract.
# kill -0 stays true for a zombie, so the producer uses the same state byte.
wait_child() {
  local child="$1"
  local prod="$2"
  local state prod_state
  while true; do
    state="$(child_state "$child" || true)"
    if [ -z "$state" ] || [ "$state" = Z ]; then
      wait "$child"
      return
    fi
    prod_state="$(child_state "$prod" || true)"
    if [ -z "$prod_state" ] || [ "$prod_state" = Z ]; then
      sleep 0.2
      state="$(child_state "$child" || true)"
      if [ -n "$state" ] && [ "$state" != Z ]; then
        kill "$child" 2>/dev/null || true
        wait "$child" 2>/dev/null || true
        return 1
      fi
      wait "$child"
      return
    fi
    sleep 0.05
  done
}

install_member() {
  local root="$1"
  local i="$2"
  local name="${mem_name[$i]}"
  local stripped="${mem_strip[$i]}"
  local actual intended
  [ -n "$stripped" ] && [ "$stripped" != "." ] || die "empty member path: $name"
  actual="$root/$stripped"
  intended="$(member_intended "$name")"
  if [ -e "$actual" ] || [ -L "$actual" ]; then
    move_member "$actual" "$intended"
  elif [[ "$name" == */ ]]; then
    mkdir -p -- "$intended"
  else
    die "member missing after extract: $name"
  fi
}

. "$_velnor_tar_here/tar-extract-plan.sh"

extract_fast() {
  if [ -n "$files_from" ]; then
    run_listed
  else
    run_busybox
  fi
}

# Leading "../" count must match. The remainder has no ".", "..", or empty part.
member_prefix_rest() {
  local name="$1"
  local out="$2"
  local n=0 value="$name" part
  while [[ "$value" == ../* ]]; do
    value="${value#../}"
    n=$((n + 1))
  done
  [ "$n" -gt 0 ] && [ -n "$value" ] || return 1
  part="$value"
  while [ -n "$part" ]; do
    case "${part%%/*}" in
      "" | . | ..) return 1 ;;
    esac
    if [ "$part" = "${part%%/*}" ]; then
      break
    fi
    part="${part#*/}"
  done
  if [ "$shared_prefix_count" -eq 0 ]; then
    shared_prefix_count=$n
  elif [ "$n" -ne "$shared_prefix_count" ]; then
    return 1
  fi
  printf -v "$out" '%s' "$value"
}

# One BusyBox extract for a shared "../" prefix. BusyBox applies a directory
# mode before its children and does not strip hardlink targets.
shared_external_base() {
  local i rest="" sample="" prefix="" k root joined intended
  shared_base=""
  shared_prefix_count=0
  [ "$extract_all" -eq 1 ] && [ "${#mem_name[@]}" -gt 0 ] || return 1
  for i in "${!mem_name[@]}"; do
    case "${mem_type[$i]}" in
      0 | 2 | 5) ;;
      *) return 1 ;;
    esac
    member_prefix_rest "${mem_name[$i]}" rest || return 1
    [ -n "$sample" ] || sample="${mem_name[$i]}"
  done
  for ((k = 0; k < shared_prefix_count; k++)); do
    prefix+="../"
  done
  root="${chdir:-$PWD}"
  norm_path "$root/$prefix" shared_base
  [ -n "$shared_base" ] || return 1
  member_prefix_rest "$sample" rest || return 1
  if [ "$shared_base" = / ]; then
    joined="/${rest%/}"
  else
    joined="$shared_base/${rest%/}"
  fi
  norm_path "$joined" joined
  member_intended "$sample" intended
  [ "$joined" = "$intended" ]
}

fail_shared_extract() {
  rm -f -- "$1"
  die "member extract failed"
}

extract_shared_external() {
  local meta
  meta="$(mktemp "${TMPDIR:-/tmp}/velnor-dir-meta.XXXXXX")"
  mkdir -p -- "$shared_base"
  stream_archive | perl /dev/fd/3 "$member_pl" "$shared_base" "$shared_prefix_count" "$meta" 3<<'PERL' | busybox tar -xf - -C "$shared_base" || fail_shared_extract "$meta"
use strict;
use warnings;
use bytes;

my ($script, $base, $count, $meta_path) = @ARGV;
die "velnor-tar-member: bad prefix\n" if !defined $meta_path || $count !~ /^\d+$/ || $count < 1;
open my $sf, "<", $script or die "velnor-tar-member: $script: $!\n";
my $src = do { local $/; <$sf> };
close $sf or die "velnor-tar-member: $script: $!\n";
my $dir = $script;
$dir =~ s{/[^/]+\z}{};
my $marker = 'use FindBin qw($RealBin);';
my $at = index $src, $marker;
die "velnor-tar-member: cannot bind modules\n" if $at < 0;
my $dir_lit = $dir;
$dir_lit =~ s/\\/\\\\/g;
$dir_lit =~ s/'/\\'/g;
substr($src, $at, length($marker)) = "my \$RealBin = '$dir_lit';";
$at = index $src, "my \$cmd = shift \@ARGV";
die "velnor-tar-member: cannot bind modules\n" if $at < 0;
substr($src, $at) = "";
eval $src;
die $@ if $@;

open my $mf, ">:raw", $meta_path or die "velnor-tar-member: $meta_path: $!\n";
my $mask = umask();
my $gwritten = 0;

sub record_dir {
    my ($name, $hdr, $state) = @_;
    my $rest = $name;
    my $left = $count;
    while ($left > 0) {
        die "velnor-tar-member: prefix\n" if $rest !~ s{\A\.\./}{};
        $left--;
    }
    $rest =~ s{/+\z}{};
    die "velnor-tar-member: empty path\n" if $rest eq "" || $rest eq ".";
    my $intended = $base eq "/" ? "/$rest" : "$base/$rest";
    my $mode = parse_size(substr($hdr, 100, 8));
    my $mtime;
    if (exists $state->{local}{mtime}) {
        $mtime = $state->{local}{mtime};
    } elsif (exists $state->{global}{mtime}) {
        $mtime = $state->{global}{mtime};
    } else {
        $mtime = parse_size(substr($hdr, 136, 12));
    }
    die "velnor-tar-member: bad mtime\n" if $mtime !~ /^\d+(?:\.\d+)?$/;
    my $perm = $mode & ~$mask;
    my $depth = ($intended =~ tr/\///);
    printf {$mf} "%s\0%o\0%.9f\0%d\0", $intended, $perm, $mtime, $depth
        or die "velnor-tar-member: write: $!\n";
}

sub patch_dir_mode {
    my ($hdr) = @_;
    my $mode = parse_size(substr($hdr, 100, 8)) | 0700;
    die "velnor-tar-member: mode overflow\n" if $mode > 07777777;
    substr($hdr, 100, 8) = sprintf("%07o\0", $mode);
    substr($hdr, 148, 8) = "        ";
    my $sum = 0;
    $sum += ord $_ for split //, $hdr;
    substr($hdr, 148, 8) = sprintf("%06o\0 ", $sum);
    return $hdr;
}

walk(
    sub {
        my ($index, $type, $name, $link, $hdr, $size, $prelude, $globals, $state) = @_;
        while ($gwritten < @$globals) {
            print STDOUT $globals->[$gwritten] or die "velnor-tar-member: write: $!\n";
            $gwritten++;
        }
        print STDOUT $prelude or die "velnor-tar-member: write: $!\n" if length $prelude;
        my $rel = $name;
        my $left = $count;
        while ($left > 0) {
            die "velnor-tar-member: prefix\n" if $rel !~ s{\A\.\./}{};
            $left--;
        }
        $rel =~ s{/+\z}{};
        my ($pax, $new) = rewrite_member($rel, $index, $hdr, $state);
        if ($type eq "5") {
            record_dir($name, $new, $state);
            $new = patch_dir_mode($new);
        }
        print STDOUT $pax or die "velnor-tar-member: write: $!\n" if $pax ne "";
        print STDOUT $new or die "velnor-tar-member: write: $!\n";
        return (\*STDOUT, undef);
    }
);
print STDOUT ("\0" x 1024) or die "velnor-tar-member: write: $!\n";
close $mf or die "velnor-tar-member: $meta_path: $!\n";
PERL
  dir_meta_file="$meta"
  restore_directory_metadata || die "cannot restore directory metadata"
  dir_metadata_restored=1
}

extract_absolute() {
  member_tool
  load_members
  if shared_external_base; then
    extract_shared_external
    return 0
  fi
  if [ "$isolated_any" -eq 0 ]; then
    extract_fast
    return 0
  fi
  reject_hardlinks
  dest_root="${chdir:-$PWD}"
  extract_planned
}

extract_archive() {
  trap cleanup_extract EXIT
  if [ "$absolute" -ne 1 ]; then
    reject_untrusted
    member_tool
    load_members
    extract_fast
    return 0
  fi
  extract_absolute
}
