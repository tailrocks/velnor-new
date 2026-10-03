#!/usr/bin/perl
# List tar members and copy one member's raw bytes. Extract stays BusyBox.
# Pax and GNU long-name headers stay attached to the member they describe.
use strict;
use warnings;
use bytes;

binmode STDOUT;

sub tell_of {
    my ($fh) = @_;
    my $pos = sysseek $fh, 0, 1;
    die "velnor-tar-member: seek: $!\n" if !defined $pos;
    return $pos;
}

sub cstr {
    my ($s) = @_;
    my $z = index $s, "\0";
    return $z < 0 ? $s : substr $s, 0, $z;
}

sub parse_size {
    my ($field) = @_;
    my $first = ord substr $field, 0, 1;
    if ($first == 0x80 || $first == 0xFF) {
        die "velnor-tar-member: negative size\n" if $first == 0xFF;
        my $n = 0;
        for my $i (1 .. 11) {
            $n = ($n * 256) + ord substr $field, $i, 1;
        }
        return $n;
    }
    $field =~ s/\0.*//s;
    $field =~ s/\s+//g;
    return 0 if $field eq "";
    die "velnor-tar-member: bad size\n" if $field !~ /^[0-7]+$/;
    return oct $field;
}

sub checksum_ok {
    my ($hdr) = @_;
    my $field = substr $hdr, 148, 8;
    $field =~ s/\0.*//s;
    $field =~ s/\s+//g;
    return 0 if $field !~ /^[0-7]+$/;
    my $copy = $hdr;
    substr($copy, 148, 8) = "        ";
    my $sum = 0;
    $sum += ord $_ for split //, $copy;
    return $sum == oct $field;
}

sub header_name {
    my ($hdr) = @_;
    my $name = cstr substr $hdr, 0, 100;
    my $magic = substr $hdr, 257, 5;
    if ($magic eq "ustar") {
        my $prefix = cstr substr $hdr, 345, 155;
        $name = "$prefix/$name" if $prefix ne "";
    }
    return $name;
}

sub parse_pax {
    my ($body) = @_;
    my %kv;
    my $i = 0;
    my $n = length $body;
    while ($i < $n) {
        my $sp = index $body, " ", $i;
        last if $sp < 0;
        my $digits = substr $body, $i, $sp - $i;
        last if $digits !~ /^\d+$/;
        my $len = int $digits;
        last if $len <= 0 || $i + $len > $n;
        my $rec = substr $body, $i, $len;
        $i += $len;
        next if $rec !~ /^(\d+) ([^=]+)=(.*)\n\z/s;
        $kv{$2} = $3;
    }
    return %kv;
}

sub read_header {
    my ($fh) = @_;
    my $at = tell_of($fh);
    my $buf;
    my $got = sysread $fh, $buf, 512;
    die "velnor-tar-member: read: $!\n" if !defined $got;
    return             if $got == 0;
    die "velnor-tar-member: short header\n" if $got != 512;
    return ($at, $buf);
}

sub read_exact {
    my ($fh, $size) = @_;
    my $data = "";
    my $left = $size;
    while ($left > 0) {
        my $buf;
        my $want = $left > 1048576 ? 1048576 : $left;
        my $got = sysread $fh, $buf, $want;
        die "velnor-tar-member: short read\n" if !defined $got || $got == 0;
        $data .= $buf;
        $left -= $got;
    }
    return $data;
}

sub skip_pad {
    my ($fh, $size) = @_;
    my $pad = (512 - ($size % 512)) % 512;
    return if $pad == 0;
    my $junk;
    my $got = sysread $fh, $junk, $pad;
    die "velnor-tar-member: short pad\n" if !defined $got || $got != $pad;
}

sub skip_rounded {
    my ($fh, $size) = @_;
    my $pad = (512 - ($size % 512)) % 512;
    my $total = $size + $pad;
    return if $total == 0;
    my $pos = tell_of($fh);
    sysseek($fh, $pos + $total, 0) or die "velnor-tar-member: seek: $!\n";
}

sub take_body {
    my ($fh, $size) = @_;
    my $data = $size > 0 ? read_exact($fh, $size) : "";
    skip_pad($fh, $size);
    return $data;
}

sub trim_nul {
    my ($s) = @_;
    $s =~ s/\0+\z//;
    return $s;
}

sub pick_name {
    my ($ustar, $gnu, $local, $global) = @_;
    return $local->{path}  if exists $local->{path};
    return $gnu             if defined $gnu;
    return $global->{path} if exists $global->{path};
    return $ustar;
}

sub meta_type {
    my ($type) = @_;
    return $type eq "g" || $type eq "x" || $type eq "X" || $type eq "L" || $type eq "K";
}

sub apply_meta {
    my ($type, $body, $state) = @_;
    if ($type eq "g") {
        my %kv = parse_pax($body);
        $state->{global}{$_} = $kv{$_} for keys %kv;
        return;
    }
    if ($type eq "x" || $type eq "X") {
        my %kv = parse_pax($body);
        $state->{local}{$_} = $kv{$_} for keys %kv;
        return;
    }
    if ($type eq "L") {
        $state->{gnu_name} = trim_nul($body);
        return;
    }
    if ($type eq "K") {
        $state->{gnu_link} = trim_nul($body);
    }
}

sub push_member {
    my ($members, $state, $at, $type, $ustar, $link, $end) = @_;
    my $name = pick_name($ustar, $state->{gnu_name}, $state->{local}, $state->{global});
    my $target = $link;
    $target = $state->{gnu_link}        if defined $state->{gnu_link};
    $target = $state->{global}{linkpath} if exists $state->{global}{linkpath} && !defined $state->{gnu_link};
    $target = $state->{local}{linkpath} if exists $state->{local}{linkpath};
    die "velnor-tar-member: member name contains a newline\n" if $name =~ /\n/ || $target =~ /\n/;
    die "velnor-tar-member: member name contains a tab\n"     if $name =~ /\t/ || $target =~ /\t/;
    my $start = defined $state->{prelude} ? $state->{prelude} : $at;
    push @$members, {
        start   => $start,
        len     => $end - $start,
        type    => $type,
        name    => $name,
        link    => $target,
        globals => $state->{gcount},
    };
    $state->{prelude}  = undef;
    $state->{local}    = {};
    $state->{gnu_name} = undef;
    $state->{gnu_link} = undef;
}

sub note_global {
    my ($globals, $state, $at, $end) = @_;
    return if defined $state->{prelude};
    push @$globals, { start => $at, len => $end - $at };
    $state->{gcount}++;
}

sub walk {
    my ($archive) = @_;
    open my $fh, "<:raw", $archive or die "velnor-tar-member: $archive: $!\n";
    my @members;
    my @globals;
    my $state = { global => {}, local => {}, gcount => 0 };
    while (1) {
        my ($at, $hdr) = read_header($fh);
        last if !defined $hdr;
        last if $hdr eq ("\0" x 512);
        die "velnor-tar-member: bad checksum at $at\n" if !checksum_ok($hdr);
        my $type = substr $hdr, 156, 1;
        $type = "0" if $type eq "\0";
        my $size = parse_size(substr $hdr, 124, 12);
        if (meta_type($type)) {
            $state->{prelude} = $at if !defined $state->{prelude} && $type ne "g";
            my $body = take_body($fh, $size);
            my $end  = tell_of($fh);
            apply_meta($type, $body, $state);
            note_global(\@globals, $state, $at, $end) if $type eq "g";
            next;
        }
        skip_rounded($fh, $size);
        push_member(
            \@members, $state, $at, $type,
            header_name($hdr), cstr(substr $hdr, 157, 100), tell_of($fh),
        );
    }
    close $fh;
    return (\@members, \@globals);
}

sub copy_range {
    my ($in, $start, $len) = @_;
    return if $len <= 0;
    sysseek($in, $start, 0) or die "velnor-tar-member: seek: $!\n";
    my $left = $len;
    while ($left > 0) {
        my $buf;
        my $want = $left > 1048576 ? 1048576 : $left;
        my $got = sysread $in, $buf, $want;
        die "velnor-tar-member: short read\n" if !defined $got || $got == 0;
        print $buf or die "velnor-tar-member: write: $!\n";
        $left -= $got;
    }
}

sub cmd_list {
    my ($archive) = @_;
    my ($members) = walk($archive);
    for my $i (0 .. $#$members) {
        my $m = $members->[$i];
        print "$i\n$m->{type}\n$m->{link}\n$m->{name}\n";
    }
}

sub read_indices {
    my ($path, $last) = @_;
    open my $fh, "<", $path or die "velnor-tar-member: $path: $!\n";
    my @want;
    while (my $line = <$fh>) {
        chomp $line;
        next if $line eq "";
        die "velnor-tar-member: bad index\n" if $line !~ /^\d+$/;
        die "velnor-tar-member: index out of range\n" if $line > $last;
        push @want, int $line;
    }
    close $fh;
    return @want;
}

sub cmd_emit {
    my ($archive, $idxfile) = @_;
    my ($members, $globals) = walk($archive);
    my @want = read_indices($idxfile, $#$members);
    open my $in, "<:raw", $archive or die "velnor-tar-member: $archive: $!\n";
    my $written = 0;
    for my $idx (@want) {
        my $m = $members->[$idx];
        while ($written < $m->{globals}) {
            my $g = $globals->[$written];
            copy_range($in, $g->{start}, $g->{len});
            $written++;
        }
        copy_range($in, $m->{start}, $m->{len});
    }
    print "\0" x 1024 or die "velnor-tar-member: write: $!\n";
    close $in;
}

my $cmd = shift @ARGV // die "velnor-tar-member: missing command\n";
if ($cmd eq "--list") {
    my $archive = shift @ARGV // die "velnor-tar-member: missing archive\n";
    cmd_list($archive);
} elsif ($cmd eq "--emit") {
    my $archive = shift @ARGV // die "velnor-tar-member: missing archive\n";
    my $idxfile = shift @ARGV // die "velnor-tar-member: missing index file\n";
    cmd_emit($archive, $idxfile);
} else {
    die "velnor-tar-member: unsupported $cmd\n";
}
