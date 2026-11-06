# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import stat
import sys
import zipfile

archive, destination, *expected = sys.argv[1:]
if len(expected) != len(set(expected)):
    raise SystemExit("duplicate expected artifact member")
with zipfile.ZipFile(archive) as source:
    entries = source.infolist()
    names = [entry.filename for entry in entries]
    if len(names) != len(set(names)) or set(names) != set(expected):
        raise SystemExit("artifact inventory mismatch")
    total = 0
    for entry in entries:
        mode = entry.external_attr >> 16
        if (not entry.filename or "/" in entry.filename or "\\" in entry.filename
                or entry.filename in (".", "..") or entry.is_dir()
                or (stat.S_IFMT(mode) not in (0, stat.S_IFREG))
                or entry.file_size <= 0 or entry.file_size > 4 * 1024**3):
            raise SystemExit("invalid artifact member")
        total += entry.file_size
        if total > 12 * 1024**3:
            raise SystemExit("artifact too large")
    for entry in entries:
        path = os.path.join(destination, entry.filename)
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        written = 0
        with source.open(entry) as input_file, os.fdopen(descriptor, "wb") as output_file:
            while True:
                block = input_file.read(1024 * 1024)
                if not block:
                    break
                written += len(block)
                if written > entry.file_size:
                    raise SystemExit("artifact member exceeded size")
                output_file.write(block)
        if written != entry.file_size:
            raise SystemExit("artifact member truncated")
