# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
from pathlib import Path
import platform
import secrets
import signal
import stat
import sys
import time

import source_preview_bootstrap as bootstrap
import source_preview_stage_verify as verifier

HANDOFF_COMMITTED = False


def host_target():
    host = (platform.system(), platform.machine())
    bootstrap.require(host in (("Linux", "x86_64"), ("Darwin", "arm64")))
    return "linux" if host == ("Linux", "x86_64") else "darwin"


def public_binding(network, source, run_id, attempt):
    bootstrap.require(bootstrap.sha40(source) and bootstrap.number(run_id) and bootstrap.number(attempt))
    repo = network.api(f"/repos/{bootstrap.REPOSITORY}")
    bootstrap.require(repo.get("id") == bootstrap.REPOSITORY_ID and repo.get("full_name") == bootstrap.REPOSITORY and repo.get("default_branch") == "main")
    bootstrap.require(network.api(f"/repos/{bootstrap.REPOSITORY}/commits/main").get("sha") == source)
    commit = network.api(f"/repos/{bootstrap.REPOSITORY}/commits/{source}")
    parents = commit.get("parents")
    bootstrap.require(commit.get("sha") == source and isinstance(parents, list) and len(parents) > 0)
    parent = parents[0].get("sha")
    bootstrap.require(bootstrap.sha40(parent))
    run = network.api(f"/repos/{bootstrap.REPOSITORY}/actions/runs/{run_id}/attempts/{attempt}")
    bootstrap.require(run.get("id") == run_id and run.get("run_attempt") == attempt and run.get("head_sha") == source)
    bootstrap.require(run.get("head_branch") == "main" and run.get("event") == "push")
    bootstrap.require(run.get("path", "").split("@", 1)[0] == ".github/workflows/product-release-images.yml")
    bootstrap.require(run.get("head_repository", {}).get("id") == bootstrap.REPOSITORY_ID and run.get("head_repository", {}).get("full_name") == bootstrap.REPOSITORY)
    bootstrap.require((run.get("status") == "in_progress" and run.get("conclusion") is None)
                      or (run.get("status") == "completed" and run.get("conclusion") == "success"))
    return parent


def verify_runtime(network, source, run_id, attempt, gh_path, temp_root):
    parent = public_binding(network, source, run_id, attempt)
    current = bootstrap.read_github_lock(network, source)
    base = bootstrap.read_github_lock(network, parent)
    preview = bootstrap.approved_preview(current, base, "runtime", source)
    bootstrap.require(preview is not None)
    ancestry = network.api(f"/repos/{bootstrap.REPOSITORY}/compare/{preview['source_sha']}...{parent}")
    bootstrap.require(ancestry.get("status") in ("ahead", "identical") and ancestry.get("merge_base_commit", {}).get("sha") == preview["source_sha"])
    authority = verifier.staging_authority()
    receipt, bundle, payloads = verifier.verify_preview(preview, gh_path, network, temp_root, authority)
    target = host_target()
    payload = payloads[target]
    bootstrap.require(hashlib.sha256(payload).hexdigest() == preview[f"{target}_payload_sha256"])
    bootstrap.require(public_binding(network, source, run_id, attempt) == parent)
    return preview, authority, target, receipt, bundle, payloads, network.deadline


def install(temp_root, preview, authority, target, receipt, bundle, payloads, deadline, emit):
    global HANDOFF_COMMITTED
    HANDOFF_COMMITTED = False
    root = Path(temp_root)
    bootstrap.require(root.is_absolute() and str(root.resolve(strict=True)) == str(root))
    root_fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        bootstrap.check_owned_directory(root_fd)
        temporary = ".source-preview-runtime-" + secrets.token_hex(16)
        signals = {signal.SIGINT, signal.SIGTERM, signal.SIGALRM}
        original_mask = signal.pthread_sigmask(signal.SIG_BLOCK, signals)
        created = False
        try:
            os.mkdir(temporary, mode=0o700, dir_fd=root_fd)
            created = True
            directory = os.open(temporary, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=root_fd)
        except BaseException:
            if created:
                os.rmdir(temporary, dir_fd=root_fd)
            signal.pthread_sigmask(signal.SIG_SETMASK, original_mask)
            raise
        committed = False
        emitted = False
        try:
            signal.pthread_sigmask(signal.SIG_SETMASK, original_mask)
            identity = {"repository": bootstrap.REPOSITORY, "version": preview["version"],
                        "source_commit": preview["source_sha"], "target": preview[f"{target}_payload_target"],
                        "asset_sha256": preview[f"{target}_payload_sha256"],
                        "authority": {"channel": "source-preview", "receipt_sha256": preview["receipt_sha256"],
                                      "bundle_sha256": preview["bundle_sha256"], "authority_sha": authority["sha"],
                                      "release_id": preview["release_id"]}}
            files = (("linux", payloads["linux"], 0o600),
                     ("darwin", payloads["darwin"], 0o600), ("receipt.json", receipt, 0o600),
                     ("bundle.json", bundle, 0o600),
                     ("identity.json", json.dumps(identity, sort_keys=True, separators=(",", ":")).encode(), 0o600))
            for name, data, mode in files:
                bootstrap.require(time.monotonic() < deadline)
                fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode, dir_fd=directory)
                with os.fdopen(fd, "wb") as output:
                    output.write(data)
                    output.flush()
                    os.fsync(output.fileno())
            os.fsync(directory)
            bootstrap.require(time.monotonic() < deadline)
            signal.pthread_sigmask(signal.SIG_BLOCK, signals)
            bootstrap.rename_exclusive(root_fd, temporary, "source-preview-runtime")
            committed = True
            bootstrap.require(time.monotonic() < deadline)
            signal.setitimer(signal.ITIMER_REAL, 0)
            emit()
            emitted = True
            HANDOFF_COMMITTED = True
            signal.pthread_sigmask(signal.SIG_SETMASK, original_mask)
        except BaseException:
            if emitted:
                raise
            signal.pthread_sigmask(signal.SIG_BLOCK, signals)
            for name in ("linux", "darwin", "receipt.json", "bundle.json", "identity.json"):
                try:
                    os.unlink(name, dir_fd=directory)
                except FileNotFoundError:
                    pass
            name = "source-preview-runtime" if committed else temporary
            path_info = os.stat(name, dir_fd=root_fd, follow_symlinks=False)
            opened = os.fstat(directory)
            bootstrap.require((path_info.st_dev, path_info.st_ino) == (opened.st_dev, opened.st_ino))
            os.rmdir(name, dir_fd=root_fd)
            signal.pthread_sigmask(signal.SIG_SETMASK, original_mask)
            raise
        finally:
            os.close(directory)
    finally:
        os.close(root_fd)
    return str(root / "source-preview-runtime")


def output_record(temp_root, preview):
    result = {"directory": str(Path(temp_root) / "source-preview-runtime"),
              "linux_sha256": preview["linux_payload_sha256"], "darwin_sha256": preview["darwin_payload_sha256"]}
    data = (json.dumps(result, sort_keys=True, separators=(",", ":")) + "\n").encode()
    fd = sys.stdout.fileno()
    bootstrap.require(stat.S_ISFIFO(os.fstat(fd).st_mode) and len(data) <= os.fpathconf(fd, "PC_PIPE_BUF"))
    return fd, data


def emit_record(fd, data):
    blocking = os.get_blocking(fd)
    os.set_blocking(fd, False)
    try:
        count = os.write(fd, data)
    finally:
        try:
            os.set_blocking(fd, blocking)
        except OSError:
            pass
    bootstrap.require(count == len(data))


def cancelled(_signum, _frame):
    if not HANDOFF_COMMITTED:
        raise bootstrap.AdmissionError("source_preview_runtime_cancelled")


def main():
    global HANDOFF_COMMITTED
    HANDOFF_COMMITTED = False
    signal.signal(signal.SIGTERM, cancelled)
    signal.signal(signal.SIGINT, cancelled)
    signal.signal(signal.SIGALRM, cancelled)
    args = sys.argv[1:]
    verify_only = bool(args and args[0] == "--verify-only")
    if verify_only:
        args = args[1:]
    bootstrap.require(len(args) == 5)
    source, run_id, attempt, gh_path, temp_root = args
    bootstrap.require(all(value.isascii() and value.isdigit() and str(int(value)) == value and bootstrap.number(int(value)) for value in (run_id, attempt)))
    token = os.environ.get("GH_TOKEN", "")
    bootstrap.require(isinstance(token, str) and bool(token))
    network = bootstrap.GitHubTransport(token)
    signal.setitimer(signal.ITIMER_REAL, max(0.001, network.deadline - time.monotonic()))
    try:
        verified = verify_runtime(network, source, int(run_id), int(attempt), gh_path, temp_root)
        preview, authority = verified[:2]
        result = {"source_sha": preview["source_sha"], "version": preview["version"],
                  "receipt_sha256": preview["receipt_sha256"], "bundle_sha256": preview["bundle_sha256"],
                  "release_id": preview["release_id"], "authority_sha": authority["sha"],
                  "linux_sha256": preview["linux_payload_sha256"], "darwin_sha256": preview["darwin_payload_sha256"]}
        if not verify_only:
            fd, data = output_record(temp_root, preview)
            install(temp_root, *verified, lambda: emit_record(fd, data))
        else:
            print(json.dumps(result, sort_keys=True, separators=(",", ":")))
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)


if __name__ == "__main__":
    try:
        main()
    except (bootstrap.AdmissionError, OSError, ValueError, KeyError):
        raise SystemExit("source preview runtime verification rejected") from None
