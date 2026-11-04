# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import pathlib
import shutil
import subprocess
import sys
import urllib.request

mode = sys.argv[1] if len(sys.argv) > 1 else "success"
env = os.environ.copy()
journal = mode in ("journal", "journal-bad-bundle")
if journal:
    env["VELNOR_FIXTURE_OFFICIAL_ASSETS"] = "/assets"
    env["VELNOR_NATIVE_JOURNAL"] = "1"
    env["VELNOR_TEST_ATTESTATION_ROOT"] = "/assets/trusted-root.jsonl"
    env["VELNOR_NATIVE_JOURNAL_ERROR"] = (
        "package_attestation_verification_failed"
        if mode == "journal-bad-bundle"
        else ""
    )
    runner = pathlib.Path("/runner") / mode
    bootstrap = runner / "velnor/bootstrap"
    bootstrap.mkdir(parents=True)
    for name in (
        "release-manifest.json",
        "release-manifest.json.intoto.jsonl",
        "velnor-actions-0.1.73-x86_64-unknown-linux-gnu",
        "velnor-actions-0.1.73-x86_64-unknown-linux-gnu.intoto.jsonl",
    ):
        shutil.copyfile(pathlib.Path("/assets") / name, bootstrap / name)
    gh_dir = runner / ".local/share/mise/installs/gh/2.103.3"
    gh_dir.mkdir(parents=True)
    shutil.copytree("/assets/gh_2.103.3_linux_amd64", gh_dir / "gh_2.103.3_linux_amd64")
    if mode == "journal-bad-bundle":
        bundle_path = bootstrap / "release-manifest.json.intoto.jsonl"
        bundle = json.loads(bundle_path.read_text())
        payload = bundle["dsseEnvelope"]["payload"]
        bundle["dsseEnvelope"]["payload"] = (
            "A" if payload[0] != "A" else "B"
        ) + payload[1:]
        bundle_path.write_text(json.dumps(bundle) + "\n")
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
        state = json.load(response)
    if mode == "journal":
        assert state["mutations"] == 3
        assert list(state["refs"]) == [
            "refs/tags/velnor-receipt/12/1/publication-started"
        ]
        record = state["records"][0]
        assert record["identity"]["workflow_sha"] == info["workflow_sha"]
        assert record["identity"]["source_sha"] == info["source_sha"]
        assert record["identity"]["run_id"] == 12
        assert record["receipt"]["generator_version"] == "0.1.73"
    else:
        assert state["mutations"] == 0
finally:
    server.terminate()
    server.wait(timeout=5)
