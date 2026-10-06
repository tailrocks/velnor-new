#!/usr/bin/perl
# One process records or restores directory mode and mtime.
# A per-directory stat, chmod, or touch fork is too slow under emulation.
use strict;
use warnings;
use bytes;
use IO::Handle;
use Time::HiRes qw(lstat utime);

my $command = shift @ARGV or die "velnor-tar-dir-meta: missing command\n";
my $state = shift @ARGV or die "velnor-tar-dir-meta: missing state\n";

if ($command eq "record") {
    record_batch($state);
} elsif ($command eq "restore") {
    exit restore_all($state);
} else {
    die "velnor-tar-dir-meta: unknown command\n";
}

sub read_record {
    my ($fh, $count) = @_;
    my $first = <$fh>;
    return if !defined $first;
    chomp $first;
    my @fields = ($first);
    for (2 .. $count) {
        my $field = <$fh>;
        return ("SHORT") if !defined $field;
        chomp $field;
        push @fields, $field;
    }
    return ("OK", @fields);
}

sub record_batch {
    my ($path) = @_;
    local $/ = "\0";
    open my $out, ">>:raw", $path or die "velnor-tar-dir-meta: $path: $!\n";
    $out->autoflush(1);
    while (1) {
        my ($status, $actual, $intended, $mode) = read_record(*STDIN, 3);
        last if !defined $status;
        die "velnor-tar-dir-meta: short record\n" if $status ne "OK";
        my @st = lstat($actual) or die "velnor-tar-dir-meta: cannot stat $actual: $!\n";
        die "velnor-tar-dir-meta: not a directory: $actual\n" if !-d _;
        my $perm = $st[2] & 07777;
        my $depth = ($intended =~ tr/\///);
        my $mtime = sprintf("%.9f", $st[9]);
        print {$out} "$intended\0$mode\0$mtime\0$depth\0"
            or die "velnor-tar-dir-meta: write: $!\n";
        chmod($perm | 0700, $actual)
            or die "velnor-tar-dir-meta: cannot defer mode $actual: $!\n";
    }
    close $out or die "velnor-tar-dir-meta: close: $!\n";
}

sub restore_all {
    my ($path) = @_;
    local $/ = "\0";
    open my $in, "<:raw", $path or die "velnor-tar-dir-meta: $path: $!\n";
    my %last;
    my $seq = 0;
    my $short = 0;
    while (1) {
        my ($status, $intended, $mode, $mtime, $depth) = read_record($in, 4);
        last if !defined $status;
        if ($status ne "OK") {
            $short = 1;
            last;
        }
        $seq += 1;
        $last{$intended} = [ $seq, $intended, $mode, $mtime, $depth + 0 ];
    }
    close $in or die "velnor-tar-dir-meta: close: $!\n";
    my @order = sort { $b->[4] <=> $a->[4] || $a->[0] <=> $b->[0] } values %last;
    my $status = 0;
    for my $item (@order) {
        my (undef, $dir, $mode, $mtime, undef) = @$item;
        next if !-d $dir || -l $dir;
        if (!chmod(oct($mode), $dir)) {
            warn "velnor-tar-dir-meta: cannot restore mode $dir: $!\n";
            $status = 1;
            next;
        }
        if (!utime($mtime, $mtime, $dir)) {
            warn "velnor-tar-dir-meta: cannot restore time $dir: $!\n";
            $status = 1;
        }
    }
    die "velnor-tar-dir-meta: short record\n" if $short;
    return $status;
}
