# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import posixpath


def valid_path(path):
    return bool(path) and not path.startswith("/") and all(
        part and part not in (".", "..") and all(ord(c) >= 32 for c in part)
        for part in path.split("/")
    )


def validate_links(links):
    for path, target in links.items():
        if not target or target.startswith("/") or any(ord(c) < 32 for c in target):
            raise ValueError("unsafe source symlink")
        seen = set()
        current = path
        value = target
        while True:
            if current in seen:
                raise ValueError("cyclic source symlink")
            seen.add(current)
            resolved = posixpath.normpath(posixpath.join(posixpath.dirname(current), value))
            if resolved == ".." or resolved.startswith("../") or resolved.startswith("/"):
                raise ValueError("source symlink escapes the archive")
            pieces = resolved.split("/")
            match = next((i for i in range(1, len(pieces) + 1) if "/".join(pieces[:i]) in links), None)
            if match is None:
                break
            current = "/".join(pieces[:match])
            value = posixpath.join(links[current], *pieces[match:])
