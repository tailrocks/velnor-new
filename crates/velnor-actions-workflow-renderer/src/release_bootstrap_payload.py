# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import hashlib
import json
import lzma
import os
import time

EXPECTED_DIGEST = '__VELNOR_PAYLOAD_SHA256__'


def check(deadline):
    if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
        raise ValueError('payload deadline')


def decode():
    payload = os.environ.pop('VELNOR_RELEASE_BOOTSTRAP_PAYLOAD')
    if (not 0 < len(payload) <= 32768
            or hashlib.sha256(payload.encode('ascii')).hexdigest() != EXPECTED_DIGEST):
        raise ValueError('payload identity')
    budget = os.environ.get('VELNOR_RELEASE_BUDGET', '').split(':')
    expected = ['v1', os.environ.get('GITHUB_RUN_ID'), os.environ.get('GITHUB_RUN_ATTEMPT'),
                os.environ.get('GITHUB_JOB')]
    if (len(budget) != 5 or budget[:4] != expected or not budget[4].isascii()
            or not budget[4].isdecimal() or not 0 < len(budget[4]) <= 20):
        raise ValueError('payload budget')
    deadline = int(budget[4])
    if not 0 < deadline < 1 << 64:
        raise ValueError('payload deadline bound')
    check(deadline)
    compressed = base64.b64decode(payload, validate=True)
    decoder = lzma.LZMADecompressor(format=lzma.FORMAT_XZ, memlimit=33554432)
    raw = bytearray()
    for offset in range(0, len(compressed), 1024):
        check(deadline)
        chunk = compressed[offset:offset + 1024]
        while True:
            raw.extend(decoder.decompress(chunk, max_length=524289 - len(raw)))
            if len(raw) > 524288 or decoder.unused_data:
                raise ValueError('payload output bound')
            check(deadline)
            if decoder.eof or decoder.needs_input:
                break
            chunk = b''
    if not decoder.eof:
        raise ValueError('payload incomplete')
    bundle = json.loads(raw)
    check(deadline)
    return bundle


try:
    bundle = decode()
except Exception:
    raise SystemExit('release bootstrap payload rejected') from None
exec(compile(bundle['stage'], '<release-bootstrap>', 'exec'), {'bundle': bundle, '__name__': '__main__'})
