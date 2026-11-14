#!/usr/bin/env python3

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
from pathlib import Path
import stat
import sys


def digest(root):
    root = Path(root)
    if not root.is_absolute() or root.is_symlink() or not root.is_dir() or root.resolve() != root:
        raise ValueError("invalid source root")
    entries = []
    seen = set()
    total = 0
    count = 0
    for directory, dirs, files in os.walk(root, followlinks=False):
        for name in dirs + files:
            path = Path(directory) / name
            rel = path.relative_to(root).as_posix()
            parts = rel.split("/")
            if not ((len(parts) == 1 and parts[0] in ("registry", "git")) or
                    (len(parts) >= 2 and tuple(parts[:2]) in (("registry", "index"), ("registry", "cache"), ("git", "db")))):
                raise ValueError("unexpected source path")
            if any(not part or part in (".", "..") or "\\" in part for part in parts):
                raise ValueError("invalid source path")
            folded = rel.casefold()
            if folded in seen:
                raise ValueError("case-folding source collision")
            seen.add(folded)
            count += 1
            if count > 10000:
                raise ValueError("source member limit")
            info = path.lstat()
            if stat.S_ISDIR(info.st_mode):
                continue
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                raise ValueError("unsafe source member")
            total += info.st_size
            if total > 536870912:
                raise ValueError("source byte limit")
            file_hash = hashlib.sha256()
            descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
            with os.fdopen(descriptor, "rb") as source:
                opened = os.fstat(source.fileno())
                if (info.st_dev, info.st_ino, info.st_mode) != (opened.st_dev, opened.st_ino, opened.st_mode):
                    raise ValueError("source member changed")
                remaining = info.st_size
                while remaining:
                    chunk = source.read(min(1048576, remaining))
                    if not chunk:
                        raise ValueError("source member truncated")
                    file_hash.update(chunk)
                    remaining -= len(chunk)
                if source.read(1):
                    raise ValueError("source member grew")
                after = os.fstat(source.fileno())
            if (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
                raise ValueError("source member changed")
            entries.append((rel, info.st_size, file_hash.hexdigest()))
    if not entries:
        raise ValueError("empty sources")
    return hashlib.sha256(json.dumps(sorted(entries), ensure_ascii=True, separators=(",", ":")).encode()).hexdigest()


if __name__ == "__main__":
    try:
        if len(sys.argv) != 2:
            raise ValueError("source directory required")
        print(digest(sys.argv[1]))
    except (OSError, UnicodeError, ValueError) as error:
        print(f"source tree: {error}", file=sys.stderr)
        sys.exit(1)
