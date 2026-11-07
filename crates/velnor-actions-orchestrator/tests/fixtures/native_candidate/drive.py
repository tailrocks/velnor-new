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
import tomllib
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
multi = mode == "coordinate-multi"
env = os.environ.copy()
coordinate = mode.startswith("coordinate")
new_run = mode == "coordinate-real-cargo-new-run"
real_cargo = mode == "coordinate-real-cargo-crash" or new_run
evidence_drift = mode in ("coordinate-evidence-drift", "coordinate-evidence-checksum-drift")
evidence_matching = mode == "coordinate-evidence-matching"
evidence_deferred = mode == "coordinate-evidence-deferred"
tamper_cargo = mode == "coordinate-cargo-tamper"
real_cargo = real_cargo or evidence_drift or evidence_matching or evidence_deferred or tamper_cargo
if real_cargo:
    env["VELNOR_FIXTURE_REAL_CARGO"] = "1"
    if (len(pathlib.Path("/proc/net/route").read_text().splitlines()) != 1
            or {name for _, name in socket.if_nameindex()} != {"lo"}):
        raise RuntimeError("real Cargo rehearsal requires a network-isolated namespace")
runner = pathlib.Path("/runner") / mode
journal = coordinate or mode in ("journal", "journal-bad-bundle")
if coordinate:
    for host in ("api.github.com", "github.com", "crates.io", "index.crates.io", "fixture.actions.githubusercontent.com") if real_cargo else ("api.github.com", "github.com", "crates.io", "fixture.actions.githubusercontent.com"):
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
    bootstrap = runner / "velnor/bootstrap"
    bootstrap.mkdir(parents=True, exist_ok=multi)
    for name in (
        "release-manifest.json",
        "release-manifest.json.intoto.jsonl",
        "velnor-actions-0.1.73-x86_64-unknown-linux-gnu",
        "velnor-actions-0.1.73-x86_64-unknown-linux-gnu.intoto.jsonl",
    ):
        shutil.copyfile(pathlib.Path("/assets") / name, bootstrap / name)
    gh_dir = runner / ".local/share/mise/installs/gh/2.103.3"
    gh_dir.mkdir(parents=True, exist_ok=multi)
    shutil.copytree("/assets/gh_2.103.3_linux_amd64", gh_dir / "gh_2.103.3_linux_amd64", dirs_exist_ok=multi)
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
            if mode in ("coordinate", "coordinate-crash", "coordinate-crash-no-credentials", "coordinate-real-cargo-crash", "coordinate-real-cargo-new-run", "coordinate-multi") or evidence_matching or evidence_deferred
            else "crate_release_toolchain_invalid"
            if tamper_cargo
            else "crate_release_semver_evidence_changed"
            if evidence_drift
            else "crate_release_upload_credential_missing"
            if mode in ("coordinate-missing-oidc", "coordinate-missing-bootstrap")
            else "crate_registry_version_not_visible"
        )
        if mode in ("coordinate-crash", "coordinate-crash-no-credentials", "coordinate-real-cargo-crash", "coordinate-real-cargo-new-run"):
            env["VELNOR_NATIVE_COORDINATE_CRASH"] = "1"
            env["VELNOR_NATIVE_PID_FILE"] = str(runner / "coordinator.pid")
            env["VELNOR_FIXTURE_CRASH_PID_PATH"] = env["VELNOR_NATIVE_PID_FILE"]
        if evidence_drift or evidence_deferred or tamper_cargo:
            env["VELNOR_NATIVE_PID_FILE"] = str(runner / "coordinator.pid")
        if tamper_cargo:
            env["VELNOR_NATIVE_TAMPER_CARGO"] = "1"
        if evidence_deferred:
            env["VELNOR_NATIVE_DEFERRED"] = "1"
        if new_run:
            env["VELNOR_NATIVE_NEW_RUN_PHASE1"] = "1"
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
        release_plz.mkdir(exist_ok=multi)
        shutil.copy("/assets/release-plz", release_plz / "release-plz")
        rust_manifest = runner / "velnor/rust-manifest"
        rust_manifest.mkdir(exist_ok=multi)
        shutil.copy("/assets/channel.toml", rust_manifest / "channel.toml")
        if mode == "coordinate-corrupt-git":
            git = runner / "velnor/git-bundle/git"
            with git.open("ab") as handle:
                handle.write(b"corrupted fixture")
if mode != "success" and not multi:
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
    if evidence_deferred:
        env["VELNOR_NATIVE_DEFERRED_FIRST"] = info["deferred_a"]
        env["VELNOR_NATIVE_DEFERRED_SECOND"] = info["deferred_b"]
        env["VELNOR_NATIVE_DEFERRED_FIRST_CHECKSUM"] = info["deferred_a_baseline_checksum"]
    tls_server = None
    registry_server = None
    if multi:
        registry_server = subprocess.Popen(
            ["python3", "/fixture/multi_registry.py", env["VELNOR_FIXTURE_FINAL_MULTI_MANIFEST"]],
            stdout=subprocess.PIPE, text=True,
        )
        registry_line = registry_server.stdout.readline()
        if not registry_line:
            raise RuntimeError("multi registry did not start")
        env["VELNOR_FIXTURE_REGISTRY_ORIGIN"] = json.loads(registry_line)["origin"]
        env["VELNOR_NATIVE_MULTI"] = "1"
    if coordinate:
        if real_cargo:
            env["VELNOR_FIXTURE_CARGO_API_ORIGIN"] = info["origin"]
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
        if not real_cargo:
            cargo = (
                runner
                / "velnor/rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/cargo"
            )
            cargo_real_path = cargo.with_name("cargo-real")
            cargo.rename(cargo_real_path)
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
                + shlex.quote(str(cargo_real_path))
                + ' package --offline "$@" >&2 || exit 73; '
                + publish_upload
                + f' || exit 75; printf "    Uploading fixture\\n    Published {info["package"]} v{info["version"]} at fixture\\n" >&2; exit 0 ;;\n'
                "  *) exec " + shlex.quote(str(cargo_real_path)) + ' "$@" ;;\n'
                "esac\n"
            )
            cargo.chmod(0o755)
    env.update(
        {
            "VELNOR_TEST_NATIVE_REHEARSAL": "1",
            "VELNOR_NATIVE_EXPECT_SUCCESS": "0"
            if mode in ("checksum", "source", "coordinate-corrupt-git", "dispatch-mismatch")
            else "1",
            "VELNOR_NATIVE_EXPECT_ERROR": {
                "checksum": "crate_release_semver_unverified",
                "source": "crate_release_source_content_mismatch",
                "coordinate-corrupt-git": "release_git_bundle_invalid",
                "dispatch-mismatch": "crate_release_dispatch_inputs",
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
            "VELNOR_TEST_REGISTRY_ORIGIN": env.get("VELNOR_FIXTURE_REGISTRY_ORIGIN", info["origin"]),
            "GITHUB_WORKSPACE": info["workspace"],
            "GITHUB_REPOSITORY": info["repository"],
            "GITHUB_SHA": info["workflow_sha"],
            "GITHUB_WORKFLOW_SHA": info["workflow_sha"],
            "GITHUB_WORKFLOW_REF": info["repository"] + "/.github/workflows/release.yml@refs/heads/main",
            "GITHUB_REF": "refs/heads/main",
            "GITHUB_EVENT_NAME": "push" if multi or evidence_deferred else "workflow_dispatch",
            "GITHUB_RUN_ID": "12",
            "GITHUB_RUN_ATTEMPT": "1",
            "GIT_TOKEN": "fixture",
            "RUNNER_TEMP": "/runner/" + mode,
        }
    )
    if not (multi or evidence_deferred):
        intent_path = pathlib.Path(info["workspace"]) / ".github/velnor-release-intent.toml"
        with intent_path.open("rb") as intent_file:
            intent = tomllib.load(intent_file)
        inputs = {"plan": "release-" + info["source_sha"][:12],
                  "source_sha": info["source_sha"], "version": info["version"]}
        if intent.get("oidc_qualification", False):
            inputs["mode"] = "release"
        if mode == "dispatch-mismatch":
            inputs["version"] = "0.0.0"
        runner.mkdir(parents=True, exist_ok=True)
        event_path = runner / "github-event.json"
        event_path.write_text(json.dumps({"inputs": inputs}))
        env["GITHUB_EVENT_PATH"] = str(event_path)
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
    if new_run:
        with urllib.request.urlopen(urllib.request.Request(
                info["origin"] + "/__fixture/advance-recovery", data=b"{}", method="POST")) as response:
            continuation = json.load(response)
        (runner / "velnor-crate-release-receipt.json").unlink()
        with urllib.request.urlopen(info["origin"] + "/__fixture/state") as response:
            after_upload = json.load(response)
        assert after_upload["uploads"] == after_upload["upload_attempts"] == 1
        env["VELNOR_NATIVE_NEW_RUN_PHASE2"] = "1"
        env["VELNOR_NATIVE_EXPECT_TARGET_CHECKSUM"] = after_upload["published_sha256"]
        env.pop("VELNOR_NATIVE_NEW_RUN_PHASE1")
        env.pop("VELNOR_NATIVE_COORDINATE_CRASH")
        env["GITHUB_SHA"] = continuation["workflow_sha"]
        env["GITHUB_WORKFLOW_SHA"] = continuation["workflow_sha"]
        env["GITHUB_RUN_ID"] = "13"
        subprocess.run(
            ["cargo", "nextest", "run", "--offline", "--locked", "-p", "velnor-actions-orchestrator", "--lib", "-E", "test(generated_candidate_receipt_rejects_unprotected_environment)", "--no-capture"],
            cwd="/src", env=env, check=True,
        )
    with urllib.request.urlopen(info["origin"] + "/__fixture/state") as response:
        state = json.load(response)
    if multi:
        assert state["uploads"] == 0
        assert "refs/tags/velnor-receipt/12/1/reconciled" in state["refs"]
    elif evidence_deferred:
        assert state["uploads"] == state["upload_attempts"] == 1
        assert f"refs/tags/{info['deferred_a']}-v0.1.1" in state["refs"]
        assert f"refs/tags/{info['deferred_b']}-v0.1.1" in state["refs"]
        assert "refs/tags/velnor-receipt/12/1/reconciled" in state["refs"]
        started = read_journal_record(info, state, "refs/tags/velnor-receipt/12/1/publication-started")
        assert started["receipt"]["semver_evidence"][info["deferred_b"]]["status"] == "deferred"
        operations = [record for record in state["records"] if record.get("mutation") is not None]
        first_metadata = [record for record in operations if record["mutation"]["package"] == info["deferred_a"] and record["mutation"]["phase"]["phase"] == "observed"]
        second_intents = [record for record in operations if record["mutation"]["package"] == info["deferred_b"] and record["mutation"]["kind"] == "upload" and record["mutation"]["phase"]["phase"] == "intended"]
        assert {record["mutation"]["kind"] for record in first_metadata} == {"create-tag", "create-release"}
        assert len(second_intents) == 1
        assert second_intents[0]["mutation"]["semver_evidence"]["status"] == "checked"
        assert second_intents[0]["mutation"]["semver_evidence"]["baseline"] == info["baseline_version"]
        assert second_intents[0]["mutation"]["semver_evidence"]["baseline_checksum"] == info["baseline_checksum"]
        assert all(record["mutation"]["sequence"] < second_intents[0]["mutation"]["sequence"] for record in first_metadata)
        with urllib.request.urlopen("https://fixture.actions.githubusercontent.com/__fixture/state") as response:
            counts = json.load(response)
        assert counts["oidc"] == counts["exchange"] == 1
    elif mode in ("coordinate", "coordinate-crash", "coordinate-crash-no-credentials", "coordinate-real-cargo-crash", "coordinate-real-cargo-new-run") or evidence_matching:
        assert state["uploads"] == 1
        if real_cargo:
            assert state["upload_attempts"] == 1
            accepted = (pathlib.Path("/workspace") / ("final-" + mode) / "fixture-upload.crate").read_bytes()
            assert hashlib.sha256(accepted).hexdigest() == state["published_sha256"]
            cargo = runner / "velnor/rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/cargo"
            package_env = {
                "PATH": str(cargo.parent) + ":/usr/bin:/bin",
                "HOME": str(runner),
                "CARGO_HOME": str(runner / "velnor/cargo"),
                "CARGO_TARGET_DIR": str(runner / "fixture-package"),
            }
            subprocess.run(
                [str(cargo), "package", "--offline", "--no-verify", "--allow-dirty"],
                cwd=pathlib.Path("/workspace") / ("final-" + mode) / "release-source",
                env=package_env, check=True, timeout=120,
            )
        packed = read_bounded(runner / "fixture-package/package" / f"{info['package']}-{info['version']}.crate", 100 * 1024 * 1024)
        assert state["published_sha256"] == hashlib.sha256(packed).hexdigest()
        assert state["release"]["tag_name"] == f"{info['package']}-v{info['version']}"
        assert state["refs"][f"refs/tags/{info['package']}-v{info['version']}"] == info["source_sha"]
        assert f"refs/tags/velnor-receipt/{13 if new_run else 12}/1/reconciled" in state["refs"]
        if evidence_matching:
            assert_pending_upload(info, state, runner)
            intent = read_journal_record(info, state, "refs/tags/velnor-receipt/12/1/operation-1")
            evidence = intent["mutation"]["semver_evidence"]
            assert evidence["status"] == "checked"
            assert evidence["baseline"] == info["baseline_version"]
            assert evidence["baseline_checksum"] == info["baseline_checksum"]
        if new_run:
            assert "refs/tags/velnor-receipt/12/1/reconciled" not in state["refs"]
            prior = read_journal_record(info, state, "refs/tags/velnor-receipt/12/1/publication-started")
            completed = read_journal_record(info, state, "refs/tags/velnor-receipt/13/1/reconciled")
            current_identity = {"repository": info["repository"], "workflow_sha": continuation["workflow_sha"],
                                "source_sha": info["source_sha"], "run_id": 13, "run_attempt": 1}
            prior_identity = {"repository": info["repository"], "workflow_sha": info["workflow_sha"],
                              "source_sha": info["source_sha"], "run_id": 12, "run_attempt": 1}
            assert prior["schema"] == completed["schema"] == 1
            assert prior["stage"] == "started" and prior["identity"] == prior_identity
            assert completed["stage"] == "completed" and completed["identity"] == current_identity
            assert completed["receipt"]["identity"] == current_identity
            assert completed["receipt"]["predecessor"] == prior_identity
            admission = json.loads((runner / "velnor-crate-release-receipt.json").read_text())
            intent = json.dumps(admission["intent"], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
            assert completed["receipt"]["intent_sha256"] == hashlib.sha256(intent).hexdigest()
            packages = completed["completion"]["packages"]
            assert len(packages) == 1
            result = packages[0]
            assert result["name"] == info["package"] and result["version"] == info["version"]
            assert result["registry_checksum"] == result["downloaded_checksum"] == state["published_sha256"]
            assert result["tag_target"] == info["source_sha"]
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
    elif tamper_cargo:
        assert state["uploads"] == state["upload_attempts"] == 0
        assert state["mutations"] == 3
        assert list(state["refs"]) == ["refs/tags/velnor-receipt/12/1/publication-started"]
        with urllib.request.urlopen("https://fixture.actions.githubusercontent.com/__fixture/state") as response:
            counts = json.load(response)
        assert counts["oidc"] == counts["exchange"] == 0
    elif evidence_drift:
        assert state["baseline_switched"]
        assert state["uploads"] == state["upload_attempts"] == 0
        assert state["mutations"] == 3
        assert list(state["refs"]) == ["refs/tags/velnor-receipt/12/1/publication-started"]
        started = read_journal_record(info, state, "refs/tags/velnor-receipt/12/1/publication-started")
        evidence = started["receipt"]["semver_evidence"][info["package"]]
        assert evidence["status"] == "checked"
        assert evidence["baseline"] == info["baseline_version"]
        assert evidence["baseline_checksum"] == info["baseline_checksum"]
        assert info["alternate_baseline_checksum"] != info["baseline_checksum"]
        if mode == "coordinate-evidence-drift":
            assert info["alternate_baseline_version"] != info["baseline_version"]
        else:
            assert info["alternate_baseline_version"] == info["baseline_version"]
        with urllib.request.urlopen("https://fixture.actions.githubusercontent.com/__fixture/state") as response:
            counts = json.load(response)
        assert counts["oidc"] == counts["exchange"] == 0
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
        if mode == "dispatch-mismatch":
            assert state["uploads"] == state["upload_attempts"] == 0
finally:
    if "registry_server" in locals() and registry_server:
        registry_server.terminate()
        registry_server.wait(timeout=5)
    if "tls_server" in locals() and tls_server:
        tls_server.terminate()
        tls_server.wait(timeout=5)
    server.terminate()
    server.wait(timeout=5)
