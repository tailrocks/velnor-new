# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import hashlib
import json
import os
import pathlib
import shlex
import shutil
import socket
import subprocess
import sys
import urllib.request

from evidence import read_bounded, write_evidence


def read_journal_record(info, state, reference):
    tag_sha = state["refs"][reference]
    root = info["origin"] + "/repos/" + info["repository"] + "/git/"
    with urllib.request.urlopen(root + "tags/" + tag_sha) as response:
        tag = json.load(response)
    tag_name = reference.removeprefix("refs/tags/")
    assert tag["sha"] == tag_sha
    assert tag["tag"] == tag_name
    assert tag["message"] == "velnor-release-receipt-v1"
    assert tag["object"]["type"] == "blob"
    blob_sha = tag["object"]["sha"]
    raw_tag = (
        f"object {blob_sha}\ntype blob\ntag {tag_name}\n"
        "tagger Velnor Release <release@velnor.invalid> 0 +0000\n\n"
        "velnor-release-receipt-v1"
    ).encode()
    assert hashlib.sha1(f"tag {len(raw_tag)}\0".encode() + raw_tag).hexdigest() == tag_sha
    with urllib.request.urlopen(root + "blobs/" + blob_sha) as response:
        blob = json.load(response)
    assert blob["sha"] == blob_sha and blob["encoding"] == "base64"
    raw = base64.b64decode(blob["content"], validate=True)
    assert len(raw) == blob["size"]
    assert hashlib.sha1(f"blob {len(raw)}\0".encode() + raw).hexdigest() == blob_sha
    return json.loads(raw)


def assert_pending_upload(info, state, runner):
    prefix = "refs/tags/velnor-receipt/12/1/"
    started = read_journal_record(info, state, prefix + "publication-started")
    pending = read_journal_record(info, state, prefix + "operation-1")
    identity = {
        "repository": info["repository"],
        "workflow_sha": info["workflow_sha"],
        "source_sha": info["source_sha"],
        "run_id": 12,
        "run_attempt": 1,
    }
    assert started["schema"] == pending["schema"] == 1
    assert started["identity"] == pending["identity"] == identity
    assert started["stage"] == "started" and started["completion"] is None
    assert pending["receipt"] == started["receipt"]
    mutation = pending["mutation"]
    assert mutation["sequence"] == 1 and mutation["kind"] == "upload"
    assert mutation["package"] == info["package"] and mutation["version"] == info["version"]
    assert mutation["phase"] == {"phase": "intended"}
    assert mutation["semver_evidence"] is not None
    admission = json.loads((runner / "velnor-crate-release-receipt.json").read_text())
    intent = json.dumps(admission["intent"], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    assert hashlib.sha256(intent).hexdigest() == pending["receipt"]["intent_sha256"]


mode = sys.argv[1] if len(sys.argv) > 1 else "success"
env = os.environ.copy()
coordinate = mode.startswith("coordinate")
journal = coordinate or mode in ("journal", "journal-bad-bundle")
if coordinate:
    for host in ("api.github.com", "github.com", "crates.io", "fixture.actions.githubusercontent.com"):
        addresses = {item[4][0] for item in socket.getaddrinfo(host, 443, type=socket.SOCK_STREAM)}
        if addresses != {"127.0.0.1"}:
            raise RuntimeError(f"{host} must resolve only to the local fixture")
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
    if coordinate:
        env["VELNOR_NATIVE_COORDINATE"] = "1"
        env["VELNOR_TEST_FAST_POLL"] = "1"
        env["VELNOR_NATIVE_COORDINATE_ERROR"] = (
            ""
            if mode in ("coordinate", "coordinate-crash", "coordinate-crash-no-credentials")
            else "crate_release_upload_credential_missing"
            if mode in ("coordinate-missing-oidc", "coordinate-missing-bootstrap")
            else "crate_registry_version_not_visible"
        )
        if mode in ("coordinate-crash", "coordinate-crash-no-credentials"):
            env["VELNOR_NATIVE_COORDINATE_CRASH"] = "1"
            env["VELNOR_NATIVE_PID_FILE"] = str(runner / "coordinator.pid")
            env["VELNOR_FIXTURE_CRASH_PID_PATH"] = env["VELNOR_NATIVE_PID_FILE"]
        if mode == "coordinate-crash-no-credentials":
            env["VELNOR_NATIVE_NO_TOKEN_ON_RETRY"] = "1"
        env["ACTIONS_ID_TOKEN_REQUEST_URL"] = (
            "https://fixture.actions.githubusercontent.com/oidc"
        )
        env["ACTIONS_ID_TOKEN_REQUEST_TOKEN"] = "fixture-oidc"
        if mode in ("coordinate-missing-oidc", "coordinate-missing-bootstrap"):
            env.pop("ACTIONS_ID_TOKEN_REQUEST_TOKEN", None)
            env.pop("CARGO_REGISTRY_TOKEN", None)
            env["VELNOR_NATIVE_POISON_CHECKER"] = "1"
        release_plz = runner / "velnor-release-plz"
        release_plz.mkdir()
        shutil.copy("/assets/release-plz", release_plz / "release-plz")
        rust_manifest = runner / "velnor/rust-manifest"
        rust_manifest.mkdir()
        shutil.copy("/assets/channel.toml", rust_manifest / "channel.toml")
        if mode == "coordinate-corrupt-git":
            git = runner / "velnor/git-bundle/git"
            with git.open("ab") as handle:
                handle.write(b"corrupted fixture")
if mode != "success":
    env["VELNOR_FIXTURE_FAILURE"] = "oidc" if mode == "coordinate-oidc-denied" else mode
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
    tls_server = None
    if coordinate:
        tls_server = subprocess.Popen(
            [
                "python3",
                "/fixture/https_fixture.py",
                str(runner / "https-fixture"),
                info["source_sha"],
            ],
            env=env,
            stdout=subprocess.PIPE,
            text=True,
        )
        if tls_server.stdout.readline().strip() != "https fixture ready":
            raise RuntimeError("HTTPS fixture did not start")
        cargo = (
            runner
            / "velnor/rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin/cargo"
        )
        real_cargo = cargo.with_name("cargo-real")
        cargo.rename(real_cargo)
        target = runner / "fixture-package"
        info_marker = runner / "release-plz-info-called"
        upload = info["origin"] + "/__fixture/upload"
        info_reply = (
            '  info) test "$(/usr/bin/readlink /proc/$PPID/exe)" = '
            + shlex.quote(str(runner / "velnor-release-plz/release-plz"))
            + ' || exit 76; : > '
            + shlex.quote(str(info_marker))
            + f'; echo "{info["package"]} {info["version"]}"; exit 0 ;;\n'
            if mode == "coordinate-skip"
            else '  info) printf "error: could not find package\\n" >&2; exit 101 ;;\n'
        )
        publish_upload = (
            ":"
            if mode == "coordinate-skip"
            else "/usr/bin/curl -fsS --data-binary @"
            + shlex.quote(str(target / "package" / f"{info['package']}-{info['version']}.crate"))
            + " "
            + shlex.quote(upload)
            + " >/dev/null"
        )
        cargo.write_text(
            "#!/bin/sh\n"
            'case "$1" in\n'
            + info_reply
            + '  publish) shift; test "${CARGO_REGISTRY_TOKEN:-}" = fixture-scoped || exit 74; '
            + "CARGO_TARGET_DIR="
            + shlex.quote(str(target))
            + " "
            + shlex.quote(str(real_cargo))
            + ' package --offline "$@" >&2 || exit 73; '
            + publish_upload
            + f' || exit 75; printf "    Uploading fixture\\n    Published {info["package"]} v{info["version"]} at fixture\\n" >&2; exit 0 ;;\n'
            "  *) exec " + shlex.quote(str(real_cargo)) + ' "$@" ;;\n'
            "esac\n"
        )
        cargo.chmod(0o755)
    env.update(
        {
            "VELNOR_TEST_NATIVE_REHEARSAL": "1",
            "VELNOR_NATIVE_EXPECT_SUCCESS": "0"
            if mode in ("checksum", "source", "coordinate-corrupt-git")
            else "1",
            "VELNOR_NATIVE_EXPECT_ERROR": {
                "checksum": "crate_release_semver_unverified",
                "source": "crate_release_source_content_mismatch",
                "coordinate-corrupt-git": "release_git_bundle_invalid",
            }.get(mode, ""),
            "VELNOR_NATIVE_VERIFY_ERROR": {
                "approval": "crate_release_environment_approval",
                "verify-source": "crate_release_source_content_mismatch",
            }.get(mode, ""),
            "VELNOR_NATIVE_EXPECT_SOURCE": info["source_sha"],
            "VELNOR_NATIVE_EXPECT_PACKAGE": info["package"],
            "VELNOR_NATIVE_EXPECT_VERSION": info["version"],
            "VELNOR_NATIVE_EXPECT_BASELINE": info["baseline_version"],
            "VELNOR_NATIVE_EXPECT_CHECKSUM": info["baseline_checksum"],
            "VELNOR_TEST_GITHUB_ORIGIN": info["origin"],
            "VELNOR_TEST_REGISTRY_ORIGIN": info["origin"],
            "GITHUB_WORKSPACE": info["workspace"],
            "GITHUB_REPOSITORY": info["repository"],
            "GITHUB_SHA": info["workflow_sha"],
            "GITHUB_WORKFLOW_SHA": info["workflow_sha"],
            "GITHUB_WORKFLOW_REF": info["repository"] + "/.github/workflows/release.yml@refs/heads/main",
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
    if mode in ("coordinate", "coordinate-crash", "coordinate-crash-no-credentials"):
        assert state["uploads"] == 1
        packed = read_bounded(runner / "fixture-package/package" / f"{info['package']}-{info['version']}.crate", 100 * 1024 * 1024)
        assert state["published_sha256"] == hashlib.sha256(packed).hexdigest()
        assert state["release"]["tag_name"] == f"{info['package']}-v{info['version']}"
        assert state["refs"][f"refs/tags/{info['package']}-v{info['version']}"] == info["source_sha"]
        assert "refs/tags/velnor-receipt/12/1/reconciled" in state["refs"]
        with urllib.request.urlopen(
            "https://fixture.actions.githubusercontent.com/__fixture/state"
        ) as response:
            counts = json.load(response)
        assert counts["oidc"] == counts["exchange"] == 1
        if mode == "coordinate":
            assert counts["revoke"] == 1
            destination = os.environ.get("VELNOR_NATIVE_FIXTURE_EVIDENCE")
            if destination:
                supplied_intent = os.environ.get("VELNOR_FIXTURE_SUPPLIED_INTENT")
                if not supplied_intent:
                    raise ValueError("fixture evidence requires a supplied intent")
                write_evidence(destination, runner, pathlib.Path("/src"), supplied_intent, info, state, counts, packed)
    elif mode == "coordinate-corrupt-git":
        assert state["uploads"] == 0
        assert state["mutations"] == 0
        assert not state["refs"]
    elif coordinate:
        assert state["uploads"] == 0
        assert state["release"] is None
        assert f"refs/tags/{info['package']}-v{info['version']}" not in state["refs"]
        assert "refs/tags/velnor-receipt/12/1/publication-started" in state["refs"]
        assert "refs/tags/velnor-receipt/12/1/reconciled" not in state["refs"]
        if mode == "coordinate-skip":
            assert info_marker.is_file()
            assert_pending_upload(info, state, runner)
        if mode in ("coordinate-missing-oidc", "coordinate-missing-bootstrap"):
            assert not any("/operation-" in ref for ref in state["refs"])
        with urllib.request.urlopen(
            "https://fixture.actions.githubusercontent.com/__fixture/state"
        ) as response:
            counts = json.load(response)
        assert counts["oidc"] == (0 if mode in ("coordinate-skip", "coordinate-missing-oidc", "coordinate-missing-bootstrap") else 1)
        assert counts["exchange"] == 0
    elif mode == "journal":
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
    if "tls_server" in locals() and tls_server:
        tls_server.terminate()
        tls_server.wait(timeout=5)
    server.terminate()
    server.wait(timeout=5)
