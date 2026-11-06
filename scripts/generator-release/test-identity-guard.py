#!/usr/bin/env python3

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import fcntl
import os
import pathlib
import select
import shutil
import subprocess
import sys
import tempfile
import time

root = pathlib.Path(__file__).resolve().parents[2]
source_guard = pathlib.Path(sys.argv[1]).resolve()
digest = sys.argv[2]
with tempfile.TemporaryDirectory(prefix="velnor-identity-guard-") as temporary:
    work = pathlib.Path(temporary).resolve()
    tree = work / "tree"
    tree.mkdir()
    for index in range(8000):
        name = f"{index:04d}-" + "a" * 100
        (tree / name).write_bytes(b"a")
        (tree / (name + "-link")).symlink_to("missing-" + "b" * 200)
    for mode in (sys.argv[3],) if len(sys.argv) > 3 else ("tree", "link"):
        for mutate in (False, True):
            directory = work / f"{mode}-{mutate}"
            directory.mkdir(mode=0o700)
            guard = directory / "guard"
            replacement = directory / "replacement"
            shutil.copyfile(source_guard, guard)
            shutil.copyfile(source_guard, replacement)
            guard.chmod(0o700)
            replacement.chmod(0o700)
            fifo = directory / "identity"
            os.mkfifo(fifo)
            env = os.environ.copy()
            env.update(ROOT=str(root), QUALIFICATION_GUARD_INPUT_PATH=str(guard),
                       QUALIFICATION_GUARD_INPUT_SHA256=digest, TREE=str(tree), FIFO=str(fifo), MODE=mode)
            script = '''source "$ROOT/scripts/generator-release/qualification-goldens.sh"
prepare_qualification_guard || exit 2
if [ "$MODE" = tree ]; then
  hash_tree "$TREE" "$FIFO"
else
  link_identity "$TREE" >"$FIFO" || exit $?
fi
printf 'IDENTITY PUBLISHED\\n'
'''
            process = subprocess.Popen(["bash", "-c", script], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            fd = os.open(fifo, os.O_RDONLY | os.O_NONBLOCK)
            if hasattr(fcntl, "F_SETPIPE_SZ"):
                fcntl.fcntl(fd, fcntl.F_SETPIPE_SZ, 4096)
            data = bytearray()
            deadline = time.monotonic() + 60
            replaced = False
            try:
                while time.monotonic() < deadline:
                    readable, _, _ = select.select([fd], [], [], 0.1)
                    if readable:
                        chunk = os.read(fd, 65536 if replaced or not mutate else 1)
                        if chunk:
                            data.extend(chunk)
                            if mutate and not replaced:
                                assert process.poll() is None, mode
                                os.replace(replacement, guard)
                                replaced = True
                        elif process.poll() is not None:
                            break
                    elif process.poll() is not None:
                        break
                else:
                    raise AssertionError(f"{mode} identity timed out")
                stdout, stderr = process.communicate(timeout=5)
                assert len(data) > 1024 * 1024, (mode, len(data), stderr)
                if mutate:
                    assert replaced and process.returncode == 2, (mode, process.returncode, stderr)
                    assert b"trusted qualification archive guard changed" in stderr, stderr
                    assert b"IDENTITY PUBLISHED" not in stdout, stdout
                else:
                    assert process.returncode == 0, (mode, process.returncode, stderr)
                    assert b"IDENTITY PUBLISHED" in stdout, stdout
            finally:
                os.close(fd)
                if process.poll() is None:
                    process.kill()
                    process.communicate()
print("passed identity guard custody and fail-closed publication")
