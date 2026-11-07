# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
from pathlib import Path
import stat
import sys
import types


def read_verified(path, expected):
    if not path.is_absolute() or len(expected) != 64 or any(c not in "0123456789abcdef" for c in expected):
        raise ValueError("invalid launcher source identity")
    for parent in (path, *path.parents):
        if stat.S_ISLNK(os.lstat(parent).st_mode):
            raise ValueError("launcher source path is a symlink")
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or before.st_size > 1_048_576:
            raise ValueError("invalid launcher source")
        with os.fdopen(descriptor, "rb", closefd=False) as source:
            data = source.read(1_048_577)
        after = os.fstat(descriptor)
        if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise ValueError("launcher source changed while read")
        if len(data) != before.st_size or hashlib.sha256(data).hexdigest() != expected:
            raise ValueError("launcher source digest mismatch")
        return data
    finally:
        os.close(descriptor)


def main():
    if len(sys.argv) != 4:
        raise ValueError("invalid launcher identity")
    root = Path(os.environ["RUNNER_TEMP"]) / "velnor" / "launcher"
    bootstrap = read_verified(root / "source_preview_bootstrap.py", sys.argv[1])
    executor = read_verified(root / "source_preview_exec.py", sys.argv[2])
    bootstrap_module = types.ModuleType("source_preview_bootstrap")
    exec(compile(bootstrap, "<verified-preview-bootstrap>", "exec"), bootstrap_module.__dict__)
    sys.modules["source_preview_bootstrap"] = bootstrap_module
    exec_module = types.ModuleType("verified_preview_executor")
    exec(compile(executor, "<verified-preview-executor>", "exec"), exec_module.__dict__)
    expected = bootstrap_module.parse_json(sys.argv[3].encode(), 4096)
    sys.argv = ["velnor-preview-launcher"]
    exec_module.main(expected)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        raise SystemExit("verified preview launcher rejected") from None
