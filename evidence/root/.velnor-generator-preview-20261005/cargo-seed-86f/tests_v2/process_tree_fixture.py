#!/usr/bin/env python3
"""Offline timeout fixture: a parent owns a sleeping child process."""

import signal
import subprocess
import sys
import time
from pathlib import Path


child = subprocess.Popen(
    [sys.executable, "-I", "-B", "-c", "import time; time.sleep(60)"],
    close_fds=True,
)
Path(sys.argv[1]).write_text(f"{child.pid}\n", encoding="ascii")


def stop_child(_signum, _frame):
    child.terminate()
    try:
        child.wait(timeout=1)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait()
    raise SystemExit(0)


signal.signal(signal.SIGTERM, stop_child)
while True:
    time.sleep(1)
