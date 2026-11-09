# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import errno
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

root = Path(sys.argv[1])
source = root / "src/native_helper"
loader = (source / "source_preview_loader.py").read_text()
original = [(root / "../velnor-actions-mise/src/source_preview/source_preview_bootstrap.py").read_bytes(), (source / "source_preview_exec.py").read_bytes()]
digests = [hashlib.sha256(value).hexdigest() for value in original]


def run_case(tampered=None, after_read=False):
    with tempfile.TemporaryDirectory() as directory:
        temp = Path(directory).resolve()
        files = temp / "velnor/launcher"
        files.mkdir(parents=True)
        names = ("source_preview_bootstrap.py", "source_preview_exec.py")
        for index, name in enumerate(names):
            (files / name).write_bytes(original[index] + (b"\n# altered" if tampered == index else b""))
        fifo = temp / "velnor/bin/controller/identity.json"
        fifo.parent.mkdir(parents=True)
        os.mkfifo(fifo)
        expected = json.dumps({"channel": "source-preview", "version": "0.1.75", "target": "x86_64-unknown-linux-gnu", "sha256": "b" * 64, "source_sha": "a" * 40, "receipt_sha256": "c" * 64}, sort_keys=True, separators=(",", ":"))
        environment = {"PATH": "/usr/bin:/bin", "RUNNER_TEMP": str(temp), "VELNOR_INTERNAL_OP": "plan-v1"}
        process = subprocess.Popen(["/usr/bin/python3", "-I", "-c", loader, *digests, expected], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, env=environment)
        opened = False
        deadline = time.monotonic() + 10
        marker = temp / "unapproved-code-executed"
        try:
            while process.poll() is None and time.monotonic() < deadline:
                try:
                    writer = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
                except OSError as error:
                    if error.errno != errno.ENXIO:
                        raise
                    time.sleep(0.01)
                else:
                    opened = True
                    if after_read:
                        for name in names:
                            (files / name).write_text(f"open({str(marker)!r}, 'w').write('bad')\n")
                    os.close(writer)
                    break
            if process.poll() is None and not opened:
                process.kill()
                process.wait()
                raise AssertionError("verified loader did not finish or reach identity input")
            _, stderr = process.communicate(timeout=10)
            if tampered is None:
                assert opened, "approved loader rejected its source modules before identity input"
                assert not marker.exists(), "replaced launcher source executed after verification"
            else:
                assert not opened and process.returncode != 0, "tampered launcher reached identity input"
                assert b"verified preview launcher rejected" in stderr
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()


run_case()
run_case(0)
run_case(1)
run_case(after_read=True)
