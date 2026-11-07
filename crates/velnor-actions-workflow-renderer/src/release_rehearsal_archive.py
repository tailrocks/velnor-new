# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import io
import os
from pathlib import Path
import stat
import subprocess
import sys
import tarfile

__VELNOR_PATHS_PYTHON__


def read_entry(root, parts, mode, limit):
    descriptors = []
    try:
        directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        descriptors.append(directory)
        for part in parts[:-1]:
            directory = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=directory)
            descriptors.append(directory)
        info = os.stat(parts[-1], dir_fd=directory, follow_symlinks=False)
        if mode == b"120000":
            if not stat.S_ISLNK(info.st_mode):
                raise ValueError("source link differs from the index")
            target = os.readlink(parts[-1], dir_fd=directory).encode("utf-8", "strict")
            after = os.stat(parts[-1], dir_fd=directory, follow_symlinks=False)
            if (info.st_dev, info.st_ino) != (after.st_dev, after.st_ino):
                raise ValueError("source link changed during read")
            return target
        if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
            raise ValueError("source is not a bounded regular file")
        descriptor = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW, dir_fd=directory)
        with os.fdopen(descriptor, "rb") as file:
            opened = os.fstat(file.fileno())
            if not stat.S_ISREG(opened.st_mode) or (info.st_dev, info.st_ino) != (opened.st_dev, opened.st_ino):
                raise ValueError("source changed during open")
            data = file.read(limit + 1)
        if len(data) > limit:
            raise ValueError("source is too large")
        return data
    finally:
        for descriptor in reversed(descriptors):
            os.close(descriptor)


def build_archive(root, archive):
    process = subprocess.Popen(
        ["git", "-C", str(root), "ls-files", "--stage", "-z"],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    )
    index = bytearray()
    count = 0
    try:
        if process.stdout is None:
            raise ValueError("source index unavailable")
        while chunk := process.stdout.read(65536):
            index.extend(chunk)
            count += chunk.count(b"\0")
            if len(index) > 16 * 1024 * 1024 or count > 100000:
                raise ValueError("source index is too large")
        if process.wait() != 0:
            raise ValueError("source index unavailable")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        if process.stdout is not None:
            process.stdout.close()
    records = index.split(b"\0")
    entries = []
    links = {}
    total = 0
    for record in records:
        if not record:
            continue
        metadata, raw_path = record.split(b"\t", 1)
        mode, oid, stage = metadata.split(b" ")
        if mode not in (b"100644", b"100755", b"120000") or stage != b"0":
            raise ValueError("submodules and unmerged source entries are unsupported")
        path = raw_path.decode("utf-8", "strict")
        if not valid_path(path):
            raise ValueError("unsafe source path")
        data = read_entry(root, path.split("/"), mode, 100 * 1024 * 1024 - total)
        if mode == b"120000":
            links[path] = data.decode("utf-8", "strict")
        total += len(data)
        if total > 100 * 1024 * 1024:
            raise ValueError("source is too large")
        if hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest().encode() != oid:
            raise ValueError("source differs from reviewed Git blob")
        entries.append((path, mode, data))
    if not entries:
        raise ValueError("source is empty")
    validate_links(links)
    with tarfile.open(archive, "x", format=tarfile.PAX_FORMAT) as tar:
        for path, mode, data in entries:
            entry = tarfile.TarInfo(path)
            if mode == b"120000":
                entry.type = tarfile.SYMTYPE
                entry.linkname = data.decode("utf-8")
                tar.addfile(entry)
            else:
                entry.size = len(data)
                entry.mode = 0o755 if mode == b"100755" else 0o644
                tar.addfile(entry, io.BytesIO(data))


if __name__ == "__main__":
    build_archive(Path(sys.argv[1]), Path(sys.argv[2]))
