# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import json
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

MODULES = str(Path(sys.argv[1]).resolve())
CHILD = r'''
import importlib.util, os, signal, sys, time
from pathlib import Path
modules, root, size = sys.argv[1], sys.argv[2], int(sys.argv[3])
for name in ("source_preview_bootstrap", "source_preview_stage_verify", "source_preview_runtime"):
    spec = importlib.util.spec_from_file_location(name, Path(modules) / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
r = sys.modules["source_preview_runtime"]
for sig in (signal.SIGTERM, signal.SIGALRM):
    signal.signal(sig, r.cancelled)
preview = {"version":"0.1.0", "source_sha":"a"*40, "linux_payload_target":"x86_64-unknown-linux-gnu", "darwin_payload_target":"aarch64-apple-darwin", "linux_payload_sha256":"b"*64, "darwin_payload_sha256":"c"*64, "receipt_sha256":"d"*64, "bundle_sha256":"e"*64, "release_id":1}
authority = {"sha":"f"*40}
fd, data = r.output_record(root, preview)
if len(sys.argv) > 4 and sys.argv[4] != "reuse":
    original_write = os.write
    def write_with_pending_signal(out, value):
        count = original_write(out, value)
        if out == fd:
            os.kill(os.getpid(), int(sys.argv[4]))
        return count
    os.write = write_with_pending_signal
payloads = {"linux":b"x"*size, "darwin":b"y"*size}
r.install(root, preview, authority, "linux", b"receipt", b"bundle", payloads, time.monotonic()+20, lambda:r.emit_record(fd,data))
if len(sys.argv) > 4 and sys.argv[4] == "reuse":
    try:
        r.install(root, preview, authority, "linux", b"receipt", b"bundle", payloads, time.monotonic()+20, lambda:r.emit_record(fd,data))
    except OSError:
        pass
    else:
        raise AssertionError("reused destination was accepted")
    try:
        r.cancelled(signal.SIGTERM, None)
    except sys.modules["source_preview_bootstrap"].AdmissionError:
        pass
    else:
        raise AssertionError("new install inherited committed cancellation state")
'''


def filled_pipe():
    read_fd, write_fd = os.pipe()
    os.set_blocking(write_fd, False)
    total = 0
    while True:
        try:
            total += os.write(write_fd, b"x" * 4096)
        except BlockingIOError:
            break
    os.set_blocking(write_fd, True)
    return read_fd, write_fd, total


def run_case(signum=None):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory).resolve()
        read_fd, write_fd, filled = filled_pipe()
        process = subprocess.Popen([sys.executable, "-B", "-I", "-c", CHILD, MODULES, str(root), str(128 * 1024 * 1024 if signum else 1)], stdout=write_fd, stderr=subprocess.PIPE)
        os.close(write_fd)
        try:
            if signum:
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    if any(path.joinpath("linux").exists() for path in root.glob(".source-preview-runtime-*")):
                        os.kill(process.pid, signum)
                        break
                    if process.poll() is not None:
                        raise AssertionError("process exited before signal during staging")
                    time.sleep(0.001)
                else:
                    raise AssertionError("staging did not begin")
            _, stderr = process.communicate(timeout=20)
            assert process.returncode != 0
            if signum:
                assert b"source_preview_runtime_cancelled" in stderr, stderr
            else:
                assert b"BlockingIOError" in stderr, stderr
            assert list(root.iterdir()) == [], "failed handoff left owned files"
            assert len(os.read(read_fd, filled + 4096)) == filled, "failed handoff emitted output"
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            os.close(read_fd)


for pending in (None, signal.SIGTERM, signal.SIGALRM, "reuse"):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory).resolve()
        command = [sys.executable, "-B", "-I", "-c", CHILD, MODULES, str(root), "1"]
        if pending:
            command.append(pending if pending == "reuse" else str(int(pending)))
        result = subprocess.run(command, capture_output=True, timeout=20)
        assert result.returncode == 0, result.stderr
        identity = json.loads(result.stdout)
        assert identity["directory"] == str(root / "source-preview-runtime")
        assert (root / "source-preview-runtime/linux").read_bytes() == b"x"
        assert (root / "source-preview-runtime/darwin").read_bytes() == b"y"

with tempfile.TemporaryDirectory() as directory:
    root = Path(directory).resolve()
    with tempfile.TemporaryFile() as output:
        result = subprocess.run([sys.executable, "-B", "-I", "-c", CHILD, MODULES, str(root), "1"], stdout=output, stderr=subprocess.PIPE, timeout=20)
    assert result.returncode != 0 and list(root.iterdir()) == [], "non-pipe output was accepted"

run_case()
run_case(signal.SIGTERM)
run_case(signal.SIGALRM)
