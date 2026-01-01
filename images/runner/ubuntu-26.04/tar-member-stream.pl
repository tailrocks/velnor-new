package Velnor::Tar::Stream;

use strict;
use warnings;

# Read past the tar end marker so upstream decompressors can reach EOF.
sub drain_stdin {
    my ($stream) = @_;
    my $buf = "";
    while (1) {
        my $got = sysread $stream, $buf, 1048576;
        die "velnor-tar-member: read: $!\n" if !defined $got;
        last if $got == 0;
    }
}

1;
