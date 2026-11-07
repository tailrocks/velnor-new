# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
from pathlib import Path
import sys
import tarfile

__VELNOR_PATHS_PYTHON__


def extract_archive(archive, destination):
    if archive.stat().st_size > 100 * 1024 * 1024:
        raise ValueError("archive is too large")
    destination.mkdir(mode=0o700)
    total = 0
    seen = set()
    links = {}
    with tarfile.open(archive, "r:") as tar:
        entries = []
        for entry in tar:
            entries.append(entry)
            if len(entries) > 100000:
                raise ValueError("invalid archive entry count")
        if not entries:
            raise ValueError("invalid archive entry count")
        for entry in entries:
            name = entry.name
            if not valid_path(name) or name in seen or not (entry.isfile() or entry.issym()):
                raise ValueError("unsafe archive entry")
            seen.add(name)
            total += entry.size
            if total > 100 * 1024 * 1024:
                raise ValueError("archive is too large")
            if entry.issym():
                links[name] = entry.linkname
        validate_links(links)
        for name in seen:
            parts = name.split("/")
            if any("/".join(parts[:i]) in links for i in range(1, len(parts))):
                raise ValueError("archive entry has a symlink ancestor")
        for entry in entries:
            if entry.issym():
                continue
            path = destination.joinpath(*entry.name.split("/"))
            path.parent.mkdir(parents=True, exist_ok=True)
            data = tar.extractfile(entry)
            if data is None:
                raise ValueError("archive entry is missing")
            with path.open("xb") as out:
                remaining = entry.size
                while remaining:
                    chunk = data.read(min(remaining, 65536))
                    if not chunk:
                        raise ValueError("archive entry is truncated")
                    out.write(chunk)
                    remaining -= len(chunk)
            os.chmod(path, 0o700 if entry.mode & 0o111 else 0o600)
        for name, target in links.items():
            path = destination.joinpath(*name.split("/"))
            path.parent.mkdir(parents=True, exist_ok=True)
            os.symlink(target, path)


if __name__ == "__main__":
    extract_archive(Path(sys.argv[1]), Path(sys.argv[2]))
