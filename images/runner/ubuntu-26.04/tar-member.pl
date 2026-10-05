#!/usr/bin/perl
# List tar members and copy selected raw bytes in one forward pass. Extraction
# stays BusyBox; pax and GNU long-name headers stay attached to their member.
use strict;
use warnings;
use bytes;
use Fcntl qw(O_WRONLY O_NONBLOCK F_GETFL F_SETFL);
use FindBin qw($RealBin);
require "$RealBin/tar-member-stream.pl";
require "$RealBin/tar-member-rewrite.pl";

binmode STDIN,  ":raw" or die "velnor-tar-member: binmode: $!\n";
binmode STDOUT;
$| = 1;

my $AT = 0;

sub counted_read {
    my ($fh, $want) = @_;
    my $buf = "";
    my $got = sysread $fh, $buf, $want;
    die "velnor-tar-member: read: $!\n" if !defined $got;
    $AT += $got if $got > 0;
    return ($got, $buf);
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
    my $buf  = "";
    my $need = 512;
    while ($need > 0) {
        my ($got, $part) = counted_read($fh, $need);
        return              if $got == 0 && $buf eq "";
        die "velnor-tar-member: short header\n" if $got == 0;
        $buf .= $part;
        $need -= $got;
    }
    return $buf;
}

sub read_exact {
    my ($fh, $size) = @_;
    my $data = "";
    my $left = $size;
    while ($left > 0) {
        my $want = $left > 1048576 ? 1048576 : $left;
        my ($got, $buf) = counted_read($fh, $want);
        die "velnor-tar-member: short read\n" if $got == 0;
        $data .= $buf;
        $left -= $got;
    }
    return $data;
}

sub transfer {
    my ($fh, $size, $out) = @_;
    return if $size <= 0;
    my $left = $size;
    while ($left > 0) {
        my $want = $left > 1048576 ? 1048576 : $left;
        my ($got, $buf) = counted_read($fh, $want);
        die "velnor-tar-member: short read\n" if $got == 0;
        if (defined $out) {
            print $out $buf or die "velnor-tar-member: write: $!\n";
        }
        $left -= $got;
    }
}

sub pad_len {
    my ($size) = @_;
    return (512 - ($size % 512)) % 512;
}

sub read_padded {
    my ($fh, $size) = @_;
    my $body = read_exact($fh, $size);
    my $padn = pad_len($size);
    my $pad  = $padn ? read_exact($fh, $padn) : "";
    return ($body, $body . $pad);
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

sub resolve_member {
    my ($state, $ustar, $link) = @_;
    my $name = pick_name($ustar, $state->{gnu_name}, $state->{local}, $state->{global});
    my $target = $link;
    $target = $state->{gnu_link} if defined $state->{gnu_link};
    $target = $state->{global}{linkpath}
        if exists $state->{global}{linkpath} && !defined $state->{gnu_link};
    $target = $state->{local}{linkpath} if exists $state->{local}{linkpath};
    die "velnor-tar-member: member name contains a newline\n" if $name =~ /\n/ || $target =~ /\n/;
    die "velnor-tar-member: member name contains a tab\n"     if $name =~ /\t/ || $target =~ /\t/;
    return ($name, $target);
}

sub open_fifo {
    my ($path) = @_;
    while (1) {
        die "velnor-tar-member: parent gone\n" if getppid() == 1;
        if (-e $path) {
            my $fh;
            if (sysopen $fh, $path, O_WRONLY | O_NONBLOCK) {
                my $flags = fcntl $fh, F_GETFL, 0;
                die "velnor-tar-member: fcntl: $!\n" if !defined $flags;
                fcntl($fh, F_SETFL, $flags & ~O_NONBLOCK)
                    or die "velnor-tar-member: fcntl: $!\n";
                binmode $fh, ":raw" or die "velnor-tar-member: binmode: $!\n";
                my $old = select $fh;
                $| = 1;
                select $old;
                return $fh;
            }
            die "velnor-tar-member: $path: $!\n" if !$!{ENXIO} && !$!{ENOENT};
        }
        select undef, undef, undef, 0.02;
    }
}

# Calls $cb->($index, $type, $name, $link, $hdr, $size, $prelude, $globals).
# $cb returns ($out, $after). $out receives the body, or undef discards it.
# $after runs after the body so a group fifo can be closed.
sub walk {
    my ($cb) = @_;
    my $state = { global => {}, local => {} };
    my @globals;
    my $prelude    = "";
    my $in_prelude = 0;
    my $index      = 0;
    while (1) {
        my $at  = $AT;
        my $hdr = read_header(\*STDIN);
        last if !defined $hdr;
        last if $hdr eq ("\0" x 512);
        die "velnor-tar-member: bad checksum at $at\n" if !checksum_ok($hdr);
        my $type = substr $hdr, 156, 1;
        $type = "0" if $type eq "\0";
        my $size = parse_size(substr $hdr, 124, 12);
        if (meta_type($type)) {
            my ($body, $padded) = read_padded(\*STDIN, $size);
            my $raw = $hdr . $padded;
            apply_meta($type, $body, $state);
            if ($type eq "g" && !$in_prelude) {
                push @globals, $raw;
            } else {
                $in_prelude = 1;
                $prelude .= $raw;
            }
            next;
        }
        my ($name, $link) = resolve_member(
            $state, header_name($hdr), cstr(substr $hdr, 157, 100),
        );
        my ($out, $after) = $cb->(
            $index, $type, $name, $link, $hdr, $size, $prelude, \@globals, $state,
        );
        transfer(\*STDIN, $size, $out);
        transfer(\*STDIN, pad_len($size), $out);
        $after->() if $after;
        $index++;
        $prelude           = "";
        $in_prelude        = 0;
        $state->{local}    = {};
        $state->{gnu_name} = undef;
        $state->{gnu_link} = undef;
    }
    Velnor::Tar::Stream::drain_stdin(\*STDIN);
    return $index;
}

sub cmd_list {
    walk(
        sub {
            my ($index, $type, $name, $link, $header) = @_;
            my $mode = sprintf "%o", parse_size(substr $header, 100, 8);
            print "$index\n$type\n$link\n$name\n$mode\n"
                or die "velnor-tar-member: write: $!\n";
            return;
        }
    );
}

sub read_groups {
    my ($path) = @_;
    open my $fh, "<", $path or die "velnor-tar-member: $path: $!\n";
    my @groups;
    my $last = -1;
    while (my $line = <$fh>) {
        chomp $line;
        next if $line eq "";
        my @idx;
        for my $tok (split / /, $line) {
            die "velnor-tar-member: bad index\n" if $tok !~ /^\d+$/;
            my $idx = int $tok;
            die "velnor-tar-member: index out of order\n" if $idx <= $last;
            $last = $idx;
            push @idx, $idx;
        }
        die "velnor-tar-member: empty group\n" if !@idx;
        push @groups, \@idx;
    }
    close $fh;
    return @groups;
}

sub wait_done {
    my ($dir) = @_;
    my $done = "$dir/done";
    while (!-e $done) {
        die "velnor-tar-member: parent gone\n" if getppid() == 1;
        select undef, undef, undef, 0.02;
    }
}

sub cmd_emit {
    my ($plan, $dir, $map_file) = @_;
    my @groups = read_groups($plan);
    my %rewrite = defined($map_file) ? read_rewrite_map($map_file) : ();
    my %seq_of;
    my %end_of;
    my $seq = 0;
    for my $g (@groups) {
        $end_of{ $g->[-1] } = 1;
        $seq_of{$_} = $seq for @$g;
        $seq++;
    }
    my $out;
    my $writing_seq = -1;
    my $gwritten    = 0;
    my $finished    = 0;
    my %rewritten;
    die "velnor-tar-member: rewrite index not selected\n"
        if grep { !exists $seq_of{$_} } keys %rewrite;
    walk(
        sub {
            my ($index, $type, $name, $link, $hdr, $size, $prelude, $globals, $state) = @_;
            return if !exists $seq_of{$index};
            my $group = $seq_of{$index};
            if (defined $out && $group != $writing_seq) {
                die "velnor-tar-member: group split\n";
            }
            if (!defined $out) {
                $out         = open_fifo("$dir/$group.fifo");
                $writing_seq = $group;
                $gwritten    = 0;
            }
            while ($gwritten < @$globals) {
                print $out $globals->[$gwritten] or die "velnor-tar-member: write: $!\n";
                $gwritten++;
            }
            if (length $prelude) {
                print $out $prelude or die "velnor-tar-member: write: $!\n";
            }
            if (exists $rewrite{$index}) {
                my ($pax, $rewritten_hdr) = rewrite_member(
                    $rewrite{$index}, $index, $hdr, $state,
                );
                print $out $pax or die "velnor-tar-member: write: $!\n" if $pax ne "";
                $hdr = $rewritten_hdr;
                $rewritten{$index} = 1;
            }
            print $out $hdr or die "velnor-tar-member: write: $!\n";
            my $after;
            if ($end_of{$index}) {
                $after = sub {
                    print $out ("\0" x 1024) or die "velnor-tar-member: write: $!\n";
                    close $out or die "velnor-tar-member: write: $!\n";
                    $out = undef;
                    $finished++;
                };
            }
            return ($out, $after);
        }
    );
    die "velnor-tar-member: index out of range\n" if $finished != @groups;
    die "velnor-tar-member: rewrite index missing\n"
        if grep { !exists $rewritten{$_} } keys %rewrite;
    wait_done($dir);
}

my $cmd = shift @ARGV // die "velnor-tar-member: missing command\n";
if ($cmd eq "--list") {
    die "velnor-tar-member: unsupported extra args\n" if @ARGV;
    cmd_list();
} elsif ($cmd eq "--emit") {
    my $plan = shift @ARGV // die "velnor-tar-member: missing plan\n";
    my $dir  = shift @ARGV // die "velnor-tar-member: missing fifo dir\n";
    my $map_file = shift @ARGV;
    die "velnor-tar-member: unsupported extra args\n" if @ARGV;
    cmd_emit($plan, $dir, $map_file);
} else {
    die "velnor-tar-member: unsupported $cmd\n";
}
