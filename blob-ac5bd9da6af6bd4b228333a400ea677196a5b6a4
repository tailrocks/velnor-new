# PAX path overrides let BusyBox extract planned -P members directly.
sub pax_record {
    my ($key, $value) = @_;
    my $body = " $key=$value\n";
    my $length = length($body) + 1;
    while (1) {
        my $record = "$length$body";
        return $record if length($record) == $length;
        $length = length $record;
    }
}

sub tar_octal {
    my ($value, $width) = @_;
    my $digits = sprintf "%0*o", $width - 1, $value;
    die "velnor-tar-member: tar field overflow\n" if length($digits) >= $width;
    return "$digits\0";
}

sub pax_path_archive {
    my ($path, $index) = @_;
    die "velnor-tar-member: unsafe rewritten path\n"
        if $path eq "" || $path eq "." || $path =~ m{\A/|(?:\A|/)\.\.(?:/|\z)};
    my $record = pax_record("path", $path);
    my $header = "\0" x 512;
    my $name = "PaxHeaders/$index";
    die "velnor-tar-member: pax header name too long\n" if length($name) > 100;
    substr($header, 0, length($name)) = $name;
    substr($header, 100, 8) = tar_octal(0644, 8);
    substr($header, 108, 8) = tar_octal(0, 8);
    substr($header, 116, 8) = tar_octal(0, 8);
    substr($header, 124, 12) = tar_octal(length($record), 12);
    substr($header, 136, 12) = tar_octal(0, 12);
    substr($header, 148, 8) = "        ";
    substr($header, 156, 1) = "x";
    substr($header, 257, 6) = "ustar\0";
    substr($header, 263, 2) = "00";
    my $sum = 0;
    $sum += ord $_ for split //, $header;
    substr($header, 148, 8) = sprintf("%06o\0 ", $sum);
    my $padding = (512 - (length($record) % 512)) % 512;
    return $header . $record . ("\0" x $padding);
}

sub rewrite_member_header {
    my ($path, $index, $header) = @_;
    die "velnor-tar-member: unsafe rewritten path\n"
        if $path eq "" || $path eq "." || $path =~ m{\A/|(?:\A|/)\.\.(?:/|\z)};
    my $name = $path;
    my $prefix = "";
    my $needs_pax = 0;
    if (length($name) > 100) {
        my $search = length($path) - 1;
        while ($search >= 0) {
            my $slash = rindex($path, "/", $search);
            last if $slash < 0;
            my $candidate_prefix = substr $path, 0, $slash;
            my $candidate_name = substr $path, $slash + 1;
            if (length($candidate_prefix) <= 155 && length($candidate_name) <= 100) {
                $prefix = $candidate_prefix;
                $name = $candidate_name;
                last;
            }
            $search = $slash - 1;
        }
        if (length($name) > 100) {
            $needs_pax = 1;
            $prefix = "";
            $name = "VelnorPax/$index";
        }
    }
    my $pax = $needs_pax ? pax_path_archive($path, $index) : "";
    substr($header, 0, 100) = "\0" x 100;
    substr($header, 0, length($name)) = $name;
    substr($header, 345, 155) = "\0" x 155;
    substr($header, 345, length($prefix)) = $prefix if $prefix ne "";
    substr($header, 257, 6) = "ustar\0";
    substr($header, 263, 2) = "00";
    substr($header, 148, 8) = "        ";
    my $sum = 0;
    $sum += ord $_ for split //, $header;
    substr($header, 148, 8) = sprintf("%06o\0 ", $sum);
    return ($pax, $header);
}

sub rewrite_member {
    my ($path, $index, $header, $state) = @_;
    my ($pax, $rewritten_header) = rewrite_member_header($path, $index, $header);
    if ($pax eq "" && (
        exists $state->{local}{path}
        || exists $state->{global}{path}
        || defined $state->{gnu_name}
    )) {
        $pax = pax_path_archive($path, $index);
    }
    return ($pax, $rewritten_header);
}

sub read_rewrite_map {
    my ($path) = @_;
    open my $fh, "<", $path or die "velnor-tar-member: $path: $!\n";
    my %map;
    while (my $line = <$fh>) {
        chomp $line;
        my ($index, $name) = split /\t/, $line, 2;
        die "velnor-tar-member: bad rewrite map\n"
            if !defined $name || $index !~ /^\d+$/ || exists $map{int $index};
        die "velnor-tar-member: unsafe rewritten path\n"
            if $name eq "" || $name eq "." || $name =~ m{\A/|(?:\A|/)\.\.(?:/|\z)};
        $map{int $index} = $name;
    }
    close $fh or die "velnor-tar-member: $path: $!\n";
    return %map;
}

1;
