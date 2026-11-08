// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#include <errno.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static _Atomic int count;
static pid_t owner;

__attribute__((constructor)) static void initialize(void) {
    owner = getpid();
}

static int replacement(pid_t pgid, int sig) {
    const char *mode = getenv("VELNOR_GROUP_PROBE_MODE");
    if (pgid >= 0 || sig != 0 || owner != getpid() || !mode) {
        return kill(pgid, sig);
    }
    int index = atomic_fetch_add(&count, 1);
    int result;
    if ((!strcmp(mode, "once") && index == 0) || !strcmp(mode, "persistent")) {
        errno = EPERM;
        result = -1;
    } else if (!strcmp(mode, "other_once") && index == 0) {
        errno = EINVAL;
        result = -1;
    } else if (!strcmp(mode, "present")) {
        result = 0;
    } else {
        result = kill(pgid, sig);
    }
    int saved = errno;
    const char *log_path = getenv("VELNOR_GROUP_PROBE_LOG");
    if (log_path) {
        FILE *log = fopen(log_path, "a");
        if (log) {
            fprintf(log, "interposed pgid=%d sig=%d index=%d result=%d errno=%d\n", pgid, sig,
                    index, result, result < 0 ? saved : 0);
            fclose(log);
        }
    }
    errno = saved;
    return result;
}

__attribute__((used)) static struct {
    const void *replacement;
    const void *original;
} interpose __attribute__((section("__DATA,__interpose"))) = {(const void *)replacement,
                                                               (const void *)kill};
