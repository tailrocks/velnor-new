# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Runs the legacy source_preview_runtime.py (verify and install) against a recorded
# GitHub, for the parity test of velnor-ci verify-runtime. Usage:
#   python3 -I -S verify_runtime.py LEGACY_DIR WORLD_JSON RUNNER_TEMP GH_PATH GH_SHA256
# Prints one JSON line: {"ok": bool, "calls": [...], "outputs": "..."}.
import copy
import json
import sys
import time
import types

legacy, world_path, runner_temp, gh_path, gh_sha = sys.argv[1:6]
sys.path.insert(0, legacy)
import source_preview_bootstrap as bootstrap
import source_preview_stage_verify as verifier
import source_preview_runtime as runtime

world = json.load(open(world_path))
calls = []


class Fake:
    def __init__(self, token):
        self.deadline = time.monotonic() + 180

    def api(self, path):
        calls.append(path)
        if path not in world["api"]:
            raise bootstrap.AdmissionError("missing " + path)
        return copy.deepcopy(world["api"][path])

    def asset(self, url, maximum):
        calls.append(url)
        if url not in world["assets"]:
            raise bootstrap.AdmissionError("missing " + url)
        body = bytes.fromhex(world["assets"][url])
        bootstrap.require(len(body) <= maximum)
        return bytearray(body)


bootstrap.GitHubTransport = Fake
fake_platform = types.SimpleNamespace(system=lambda: "Linux", machine=lambda: "x86_64")
verifier.platform = fake_platform
runtime.platform = fake_platform
verifier.GH_SHA256 = {("Linux", "x86_64"): {gh_sha}}

ok = False
outputs = ""
try:
    verified = runtime.verify_runtime(Fake(""), world["public_source"], world["public_run"], 1, gh_path, runner_temp)
    preview = verified[0]
    runtime.install(runner_temp, *verified, lambda: None)
    outputs = "linux_sha256=%s\ndarwin_sha256=%s\n" % (preview["linux_payload_sha256"], preview["darwin_payload_sha256"])
    ok = True
except (bootstrap.AdmissionError, OSError, ValueError, KeyError):
    pass
print(json.dumps({"ok": ok, "calls": calls, "outputs": outputs}))
