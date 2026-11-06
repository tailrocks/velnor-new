# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import stat
import sys
import types


def main():
    if len(sys.argv) != 4 or not os.path.isabs(os.environ["RUNNER_TEMP"]):
        raise ValueError("invalid preview setup")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    root = os.open(os.environ["RUNNER_TEMP"], flags)
    velnor = -1
    launcher = -1
    try:
        info = os.fstat(root)
        if info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ValueError("unsafe runner temporary directory")
        try:
            os.mkdir("velnor", 0o700, dir_fd=root)
        except FileExistsError:
            pass
        velnor = os.open("velnor", flags, dir_fd=root)
        info = os.fstat(velnor)
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ValueError("unsafe preview parent")
        os.mkdir("launcher", 0o700, dir_fd=velnor)
        launcher = os.open("launcher", flags, dir_fd=velnor)
        for name, encoded in zip(("source_preview_bootstrap.py", "source_preview_exec.py"), sys.argv[1:3], strict=True):
            data = bytes.fromhex(encoded)
            if not data or len(data) > 1_048_576:
                raise ValueError("invalid preview source")
            fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=launcher)
            with os.fdopen(fd, "wb") as output:
                output.write(data)
                output.flush()
                os.fsync(output.fileno())
        bootstrap = types.ModuleType("source_preview_bootstrap")
        exec(compile(bytes.fromhex(sys.argv[1]), "<protected-preview-bootstrap>", "exec"), bootstrap.__dict__)
        expected = bootstrap.parse_json(bytes.fromhex(sys.argv[3]), 4096)
        context = bootstrap.environment_context()
        bootstrap.authorize_and_acquire(context, bootstrap.GitHubTransport(os.environ.get("GH_TOKEN", "")), expected)
    finally:
        if launcher >= 0:
            os.close(launcher)
        if velnor >= 0:
            os.close(velnor)
        os.close(root)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        raise SystemExit("preview setup rejected") from None
