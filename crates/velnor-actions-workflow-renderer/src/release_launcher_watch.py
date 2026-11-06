# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import select
import struct
import time


def watch():
    data = bytearray()
    while len(data) < 16:
        part = os.read(0, 16 - len(data))
        if not part:
            raise ValueError('launcher closed')
        data.extend(part)
    deadline, namespace = struct.unpack('>QQ', data)
    if namespace != os.stat('/proc/self/ns/time').st_ino:
        raise ValueError('launcher namespace')
    while True:
        remaining = deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)
        if remaining <= 0:
            raise ValueError('launcher deadline')
        if select.select([0], [], [], min(remaining / 1_000_000_000, 0.1))[0]:
            os.read(0, 1)
            raise ValueError('launcher closed or sent unexpected input')


if __name__ == '__main__':
    try:
        watch()
    except (OSError, ValueError):
        raise SystemExit('launcher liveness ended') from None
