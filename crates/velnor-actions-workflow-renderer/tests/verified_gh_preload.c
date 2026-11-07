// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#include <fcntl.h>
#include <stdlib.h>
#include <unistd.h>

__attribute__((constructor)) static void observe(void) {
    const char *marker = getenv("MARKER");
    if (marker && getenv("GH_TOKEN")) {
        int fd = open(marker, O_CREAT | O_WRONLY | O_CLOEXEC, 0600);
        if (fd >= 0) close(fd);
    }
}
