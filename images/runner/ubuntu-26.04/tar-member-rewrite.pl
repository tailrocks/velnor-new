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

sub pax_meta_archive {
    my ($index, @pairs) = @_;
    my $body = "";
    while (@pairs) {
        my ($key, $value) = splice @pairs, 0, 2;
        $body .= pax_record($key, $value);
    }
    my $header = "\0" x 512;
    my $name = "PaxHeaders/$index";
    die "velnor-tar-member: pax header name too long\n" if length($name) > 100;
    substr($header, 0, length($name)) = $name;
    substr($header, 100, 8) = tar_octal(0644, 8);
    substr($header, 108, 8) = tar_octal(0, 8);
    substr($header, 116, 8) = tar_octal(0, 8);
    substr($header, 124, 12) = tar_octal(length($body), 12);
    substr($header, 136, 12) = tar_octal(0, 12);
    substr($header, 148, 8) = "        ";
    substr($header, 156, 1) = "x";
    substr($header, 257, 6) = "ustar\0";
    substr($header, 263, 2) = "00";
    my $sum = 0;
    $sum += ord $_ for split //, $header;
    substr($header, 148, 8) = sprintf("%06o\0 ", $sum);
    my $padding = (512 - (length($body) % 512)) % 512;
    return $header . $body . ("\0" x $padding);
}

sub remove_pax_linkpath {
    my ($raw, $header, $body) = @_;
    my ($filtered, $changed) = ("", 0);
    my $i = 0;
    my $n = length $body;
    while ($i < $n) {
        my $sp = index $body, " ", $i;
        die "velnor-tar-member: The PAX record is invalid.\n" if $sp < 0;
        my $digits = substr $body, $i, $sp - $i;
        die "velnor-tar-member: The PAX record length is invalid.\n"
            if $digits !~ /^\d+$/;
        my $length = int $digits;
        die "velnor-tar-member: The PAX record length is invalid.\n"
            if $length <= 0 || $i + $length > $n;
        my $record = substr $body, $i, $length;
        $i += $length;
        die "velnor-tar-member: The PAX record is invalid.\n"
            if $record !~ /^(\d+) ([^=\0]+)=([^\0]*)\n\z/s || int($1) != $length;
        if ($2 eq "linkpath") {
            $changed = 1;
        } else {
            $filtered .= $record;
        }
    }
    return $raw if !$changed;
    return "" if $filtered eq "";

    my $rewritten_header = $header;
    substr($rewritten_header, 124, 12) = tar_octal(length($filtered), 12);
    substr($rewritten_header, 148, 8) = "        ";
    my $sum = 0;
    $sum += ord $_ for split //, $rewritten_header;
    substr($rewritten_header, 148, 8) = sprintf("%06o\0 ", $sum);
    my $padding = (512 - (length($filtered) % 512)) % 512;
    return $rewritten_header . $filtered . ("\0" x $padding);
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
    return pax_meta_archive($index, "path", $path);
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

sub rewrite_member_with_link {
    my ($path, $link, $index, $header, $state, $canonical_link) = @_;
    my ($pax, $rewritten_header) = rewrite_member($path, $index, $header, $state);
    my $needs_link_pax = length($link) > 100
        || (!$canonical_link && (
            exists $state->{local}{linkpath}
            || exists $state->{global}{linkpath}
            || defined $state->{gnu_link}
        ));
    if ($needs_link_pax) {
        if ($pax ne "") {
            $pax = pax_meta_archive($index, "path", $path, "linkpath", $link);
        } else {
            $pax = pax_meta_archive($index, "linkpath", $link);
        }
        $link = "VelnorLink/$index" if length($link) > 100;
    }
    substr($rewritten_header, 157, 100) = "\0" x 100;
    substr($rewritten_header, 157, length($link)) = $link if $link ne "";
    substr($rewritten_header, 148, 8) = "        ";
    my $sum = 0;
    $sum += ord $_ for split //, $rewritten_header;
    substr($rewritten_header, 148, 8) = sprintf("%06o\0 ", $sum);
    return ($pax, $rewritten_header);
}

sub read_rewrite_map {
    my ($path) = @_;
    open my $fh, "<", $path or die "velnor-tar-member: $path: $!\n";
    my %map;
    while (my $line = <$fh>) {
        chomp $line;
        my ($index, $name, $link) = split /\t/, $line, 3;
        die "velnor-tar-member: bad rewrite map\n"
            if !defined $name || $index !~ /^\d+$/ || exists $map{int $index};
        die "velnor-tar-member: unsafe rewritten path\n"
            if $name eq "" || $name eq "." || $name =~ m{\A/|(?:\A|/)\.\.(?:/|\z)};
        $map{int $index} = [$name, $link];
    }
    close $fh or die "velnor-tar-member: $path: $!\n";
    return %map;
}

1;
