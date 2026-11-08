#!/usr/bin/python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import subprocess
import sys
import threading

provider = subprocess.Popen([os.environ["VELNOR_FIXTURE_REAL_PROVIDER"], *sys.argv[1:]], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
trace = os.environ["VELNOR_FIXTURE_PROVIDER_TRACE"]


def note(value):
    with open(trace, "a", encoding="utf-8") as output:
        output.write(value + "\n")


def forward_requests():
    while line := sys.stdin.buffer.readline(8193):
        if len(line) > 8192 or not line.endswith(b"\n"):
            break
        try:
            request = json.loads(line)
            if request.get("kind") == "get" and request.get("operation") == "read":
                note("read")
        except (ValueError, AttributeError):
            pass
        provider.stdin.write(line)
        provider.stdin.flush()
    provider.stdin.close()


thread = threading.Thread(target=forward_requests, daemon=True)
thread.start()
while line := provider.stdout.readline(8193):
    if len(line) > 8192 or not line.endswith(b"\n"):
        break
    try:
        response = json.loads(line)
        if response == {"Ok": {"kind": "get", "token": "velnor-nonsecret-read-placeholder", "cache": "never", "operation_independent": False}}:
            note("placeholder")
    except ValueError:
        pass
    sys.stdout.buffer.write(line)
    sys.stdout.buffer.flush()
thread.join(timeout=1)
sys.exit(provider.wait(timeout=30))
