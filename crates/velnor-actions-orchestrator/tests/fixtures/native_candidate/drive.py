# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import subprocess
import sys
import urllib.request

mode = sys.argv[1] if len(sys.argv) > 1 else "success"
env = os.environ.copy()
if mode != "success":
    env["VELNOR_FIXTURE_FAILURE"] = mode
fixture_env = env.copy()
fixture_env.pop("CARGO_TARGET_DIR", None)
server = subprocess.Popen(
    ["python3", "/fixture/fixture.py", "/workspace/final-" + mode],
    env=fixture_env,
    stdout=subprocess.PIPE,
    text=True,
)
try:
    line = server.stdout.readline()
    if not line:
        raise RuntimeError("fixture did not start")
    info = json.loads(line)
    env.update(
        {
            "VELNOR_TEST_NATIVE_REHEARSAL": "1",
            "VELNOR_NATIVE_EXPECT_SUCCESS": "0"
            if mode in ("checksum", "source")
            else "1",
            "VELNOR_NATIVE_EXPECT_ERROR": {
                "checksum": "crate_release_semver_unverified",
                "source": "crate_release_source_content_mismatch",
            }.get(mode, ""),
            "VELNOR_NATIVE_VERIFY_ERROR": {
                "approval": "crate_release_environment_approval",
                "verify-source": "crate_release_source_content_mismatch",
            }.get(mode, ""),
            "VELNOR_NATIVE_EXPECT_SOURCE": info["source_sha"],
            "VELNOR_NATIVE_EXPECT_PACKAGE": info["package"],
            "VELNOR_NATIVE_EXPECT_VERSION": info["version"],
            "VELNOR_NATIVE_EXPECT_BASELINE": "0.1.0",
            "VELNOR_NATIVE_EXPECT_CHECKSUM": info["baseline_checksum"],
            "VELNOR_TEST_GITHUB_ORIGIN": info["origin"],
            "VELNOR_TEST_REGISTRY_ORIGIN": info["origin"],
            "GITHUB_WORKSPACE": info["workspace"],
            "GITHUB_REPOSITORY": "owner/project",
            "GITHUB_SHA": info["workflow_sha"],
            "GITHUB_WORKFLOW_SHA": info["workflow_sha"],
            "GITHUB_WORKFLOW_REF": "owner/project/.github/workflows/release.yml@refs/heads/main",
            "GITHUB_REF": "refs/heads/main",
            "GITHUB_EVENT_NAME": "workflow_dispatch",
            "GITHUB_RUN_ID": "12",
            "GITHUB_RUN_ATTEMPT": "1",
            "GIT_TOKEN": "fixture",
            "RUNNER_TEMP": "/runner/" + mode,
        }
    )
    if mode == "verify-source":
        env["VELNOR_NATIVE_MUTATE_AFTER_RECEIPT"] = "1"
    subprocess.run(
        [
            "cargo",
            "nextest",
            "run",
            "--offline",
            "--locked",
            "-p",
            "velnor-actions-orchestrator",
            "--lib",
            "-E",
            "test(generated_candidate_receipt_rejects_unprotected_environment)",
            "--no-capture",
        ],
        cwd="/src",
        env=env,
        check=True,
    )
    with urllib.request.urlopen(info["origin"] + "/__fixture/state") as response:
        assert json.load(response)["mutations"] == 0
finally:
    server.terminate()
    server.wait(timeout=5)
