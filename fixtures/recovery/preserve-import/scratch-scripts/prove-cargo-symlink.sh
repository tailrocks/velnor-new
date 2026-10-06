#!/bin/bash
# Run /usr/bin/tar from the worker image. Do not call a host copy of the shim.
set -eu
echo "tar=$(readlink -f /usr/bin/tar)"
grep -n '^job_path()' /usr/local/bin/tar-absolute.sh
repo=/home/runner/work/repo/repo
rm -rf /home/runner/work
mkdir -p "$repo"
perl - /tmp/rls.tar 's' '../../_temp/velnor/cargo/bin/rls' 'rustup' << 'END_PERL'
use strict;
use warnings;
use bytes;
my ($out, $kind, $name, $target) = @ARGV;
open my $fh, ">:raw", $out or die $!;
select $fh;
my $buf = "\0" x 512;
die "name too long\n" if length($name) > 100;
substr($buf, 0, length($name)) = $name;
substr($buf, 100, 7) = sprintf("%07o", 0644);
substr($buf, 108, 7) = sprintf("%07o", 0);
substr($buf, 116, 7) = sprintf("%07o", 0);
substr($buf, 124, 11) = sprintf("%011o", 0);
substr($buf, 136, 11) = sprintf("%011o", 0);
substr($buf, 148, 8) = "        ";
substr($buf, 156, 1) = "2";
substr($buf, 157, length($target)) = $target;
substr($buf, 257, 6) = "ustar\0";
substr($buf, 263, 2) = "00";
my $sum = 0;
$sum += ord($_) for split //, $buf;
substr($buf, 148, 8) = sprintf("%06o\0 ", $sum);
print $buf or die $!;
print "\0" x 1024 or die $!;
END_PERL
/usr/bin/tar -xf /tmp/rls.tar -P -C "$repo"
link=/home/runner/work/_temp/velnor/cargo/bin/rls
test -L "$link"
test "$(readlink "$link")" = rustup
echo "rls_ok path=$link target=$(readlink "$link")"
perl - /tmp/bad.tar 's' '../../_temp/velnor/cargo/bin/bad' '/etc/passwd' << 'END_PERL'
use strict;
use warnings;
use bytes;
my ($out, $kind, $name, $target) = @ARGV;
open my $fh, ">:raw", $out or die $!;
select $fh;
my $buf = "\0" x 512;
substr($buf, 0, length($name)) = $name;
substr($buf, 100, 7) = sprintf("%07o", 0644);
substr($buf, 108, 7) = sprintf("%07o", 0);
substr($buf, 116, 7) = sprintf("%07o", 0);
substr($buf, 124, 11) = sprintf("%011o", 0);
substr($buf, 136, 11) = sprintf("%011o", 0);
substr($buf, 148, 8) = "        ";
substr($buf, 156, 1) = "2";
substr($buf, 157, length($target)) = $target;
substr($buf, 257, 6) = "ustar\0";
substr($buf, 263, 2) = "00";
my $sum = 0;
$sum += ord($_) for split //, $buf;
substr($buf, 148, 8) = sprintf("%06o\0 ", $sum);
print $buf or die $!;
print "\0" x 1024 or die $!;
END_PERL
set +e
/usr/bin/tar -xf /tmp/bad.tar -P -C "$repo" >/tmp/bad.err 2>&1
status=$?
set -e
echo "bad_status=$status"
grep -F 'escapes' /tmp/bad.err
test "$status" -ne 0
test ! -e /home/runner/work/_temp/velnor/cargo/bin/bad
test ! -L /etc/passwd
echo EXTRACT_OK
