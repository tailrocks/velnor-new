#!/usr/bin/perl
# Uncompressed ustar that keeps the caller's member names.
# BusyBox strips leading "/" and ".." on create, so a GNU -P name such as
# ../../../.cache/... cannot be stored by BusyBox. Names that fit in the
# ustar field are written there. Longer names get a pax path header.
use strict;
use warnings;
use bytes;
binmode STDOUT;

my $chdir = "";
my $files_from = "";
my @paths;

while (@ARGV) {
    my $arg = shift @ARGV;
    if ($arg eq "--chdir") {
        $chdir = shift @ARGV // die "velnor-tar-pax: missing --chdir\n";
        next;
    }
    if ($arg eq "--files-from") {
        $files_from = shift @ARGV // die "velnor-tar-pax: missing --files-from\n";
        next;
    }
    if ($arg eq "--") {
        push @paths, @ARGV;
        last;
    }
    if ($arg =~ /^-/) {
        die "velnor-tar-pax: unsupported $arg\n";
    }
    push @paths, $arg;
}

if ($files_from ne "") {
    open my $fh, "<", $files_from or die "velnor-tar-pax: $files_from: $!\n";
    while (my $line = <$fh>) {
        chomp $line;
        push @paths, $line if $line ne "";
    }
    close $fh;
}

die "velnor-tar-pax: no paths\n" unless @paths;

my %seen;

sub octal {
    my ($value, $width) = @_;
    die "velnor-tar-pax: value does not fit in $width bytes\n"
        if $value < 0 || length(sprintf("%o", $value)) > $width - 1;
    sprintf("%0*o", $width - 1, $value);
}

sub header {
    my ($name, $size, $mode, $mtime, $type, $link) = @_;
    $link //= "";
    my $shown = length($name) > 100 ? substr($name, -100) : $name;
    my $link_shown = length($link) > 100 ? substr($link, 0, 100) : $link;
    my $buf = pack(
        "a100 a8 a8 a8 a12 a12 a8 a1 a100 a6 a2 a32 a32 a8 a8 a155 a12",
        $shown,
        octal($mode, 8),
        octal(0, 8),
        octal(0, 8),
        octal($size, 12),
        octal($mtime, 12),
        "        ",
        $type,
        $link_shown,
        "ustar\0",
        "00",
        "root",
        "root",
        "",
        "",
        "",
        "",
    );
    die "velnor-tar-pax: header length " . length($buf) . "\n" if length($buf) != 512;
    my $sum = 0;
    $sum += ord($_) for split //, $buf;
    substr($buf, 148, 8) = sprintf("%06o\0 ", $sum);
    return $buf;
}

sub pax_record {
    my ($key, $value) = @_;
    my $rest = " $key=$value\n";
    my $digits = 1;
    while (1) {
        my $total = $digits + length($rest);
        my $width = length($total);
        return $total . $rest if $width == $digits;
        $digits = $width;
        die "velnor-tar-pax: pax record too large\n" if $digits > 12;
    }
}

sub write_pad {
    my ($size) = @_;
    my $pad = (512 - ($size % 512)) % 512;
    print "\0" x $pad if $pad;
}

sub emit_meta {
    my ($name, $type, $mode, $mtime, $size, $link) = @_;
    $link //= "";
    my $pax = "";
    if (length($name) > 100) {
        $pax = pax_record("path", $name);
    }
    if (length($link) > 100) {
        $pax .= pax_record("linkpath", $link);
    }
    if ($pax ne "") {
        print header("./PaxHeaders.0/member", length($pax), 0644, $mtime, "x", "")
            or die "velnor-tar-pax: write: $!\n";
        print $pax or die "velnor-tar-pax: write: $!\n";
        write_pad(length($pax));
    }
    print header($name, $size, $mode, $mtime, $type, $link)
        or die "velnor-tar-pax: write: $!\n";
}

sub source_of {
    my ($stored) = @_;
    return $stored if $stored =~ m{^/};
    return $chdir eq "" ? $stored : "$chdir/$stored";
}

sub add_path {
    my ($stored) = @_;
    die "velnor-tar-pax: member name contains a newline\n" if $stored =~ /\n/;
    die "velnor-tar-pax: member name contains a tab\n"     if $stored =~ /\t/;
    return if $seen{$stored}++;
    my $source = source_of($stored);
    my @st = lstat $source or die "velnor-tar-pax: $stored: $!\n";
    my $mode = $st[2] & 07777;
    my $mtime = $st[9];
    if (-l _) {
        my $target = readlink $source;
        defined $target or die "velnor-tar-pax: $stored: $!\n";
        emit_meta($stored, "2", $mode, $mtime, 0, $target);
        return;
    }
    if (-d _) {
        emit_meta($stored, "5", $mode, $mtime, 0, "");
        opendir my $dh, $source or die "velnor-tar-pax: $stored: $!\n";
        my @kids = sort grep { $_ ne "." && $_ ne ".." } readdir $dh;
        closedir $dh;
        my $prefix = $stored;
        $prefix =~ s{/$}{};
        add_path("$prefix/$_") for @kids;
        return;
    }
    if (-f _) {
        my $size = $st[7];
        emit_meta($stored, "0", $mode, $mtime, $size, "");
        open my $in, "<:raw", $source or die "velnor-tar-pax: $stored: $!\n";
        my $left = $size;
        while ($left > 0) {
            my $want = $left > 1048576 ? 1048576 : $left;
            my $buf;
            my $got = read $in, $buf, $want;
            die "velnor-tar-pax: $stored: short read\n" if !defined $got || $got == 0;
            print $buf or die "velnor-tar-pax: write: $!\n";
            $left -= $got;
        }
        close $in;
        write_pad($size);
        return;
    }
    die "velnor-tar-pax: unsupported file type: $stored\n";
}

add_path($_) for @paths;
print "\0" x 1024;
