# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
from pathlib import Path
import platform
import selectors
import signal
import subprocess
import sys
import tempfile
import time

import source_preview_bootstrap as bootstrap

GH_SHA256 = {
    ("Linux", "x86_64"): {"cbfdc94f3a5c0713c0d6df3e0a1d88a59a76b0c52332aad7d21d112f9da2cc0e", "b768184570b12536be4ac1528893f6340d1c6a7b40b56ad45bca9cd0e87ad34d"},
    ("Darwin", "arm64"): {"5e4470bb57bf1e7cb5063be940d5dffc5be3e1899d6bf7fe1ff15372f7f4cc29", "5f8d7527e2bdf584249b030cdd1550c91d75e8afb40aa3c11573d416817a1260"},
}
OWNER_ID = "78806509"
MAX_GH_BYTES = 104_857_600
MAX_VERIFY_OUTPUT = 16_777_216


def verify_gh_binary(path):
    expected = GH_SHA256.get((platform.system(), platform.machine()))
    bootstrap.require(expected is not None)
    data = bootstrap.read_regular(path, MAX_GH_BYTES)
    bootstrap.require(hashlib.sha256(data).hexdigest() in expected)
    bootstrap.require(os.access(path, os.X_OK))


def invoke_gh(path, receipt_path, bundle_path, source):
    environment = {key: os.environ[key] for key in ("HOME", "LANG", "LC_ALL", "TMPDIR") if key in os.environ}
    environment.update({"PATH": "/usr/bin:/bin", "GH_PROMPT_DISABLED": "1"})
    command = [path, "attestation", "verify", str(receipt_path), "--bundle", str(bundle_path), "--repo", bootstrap.REPOSITORY, "--signer-workflow", bootstrap.REPOSITORY + "/.github/workflows/product-release-generator.yml", "--signer-digest", source, "--source-digest", source, "--source-ref", "refs/tags/sdk-preview-" + source, "--format", "json"]
    process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=environment, start_new_session=True)
    output = bytearray()
    deadline = time.monotonic() + 90
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                bootstrap.require(remaining > 0)
                ready = selector.select(remaining)
                bootstrap.require(bool(ready))
                for key, _ in ready:
                    chunk = os.read(key.fd, min(65_536, MAX_VERIFY_OUTPUT + 1 - len(output)))
                    if not chunk:
                        selector.unregister(key.fileobj)
                    else:
                        output.extend(chunk)
                        bootstrap.require(len(output) <= MAX_VERIFY_OUTPUT)
        bootstrap.require(process.wait(timeout=max(0.001, deadline - time.monotonic())) == 0)
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
    return bootstrap.parse_json(output, MAX_VERIFY_OUTPUT)


def certificate_matches(certificate, source, workflow, run_id, attempt):
    uri = f"https://github.com/{bootstrap.REPOSITORY}/{workflow}@refs/tags/sdk-preview-{source}"
    expected = {
        "issuer": "https://token.actions.githubusercontent.com",
        "subjectAlternativeName": uri,
        "buildSignerURI": uri,
        "buildSignerDigest": source,
        "buildConfigURI": uri,
        "buildConfigDigest": source,
        "sourceRepositoryURI": f"https://github.com/{bootstrap.REPOSITORY}",
        "sourceRepositoryIdentifier": str(bootstrap.REPOSITORY_ID),
        "sourceRepositoryOwnerURI": "https://github.com/tailrocks",
        "sourceRepositoryOwnerIdentifier": OWNER_ID,
        "sourceRepositoryDigest": source,
        "sourceRepositoryRef": "refs/tags/sdk-preview-" + source,
        "buildTrigger": "push",
        "runnerEnvironment": "github-hosted",
        "runInvocationURI": f"https://github.com/{bootstrap.REPOSITORY}/actions/runs/{run_id}/attempts/{attempt}",
    }
    return isinstance(certificate, dict) and all(certificate.get(key) == value for key, value in expected.items())


def verify_attestation(path, receipt, bundle, preview, temp_root):
    verify_gh_binary(path)
    with tempfile.TemporaryDirectory(prefix="source-preview-stage-", dir=temp_root) as temporary:
        root = Path(temporary)
        receipt_path = root / "SDK_PREVIEW_RECEIPT.json"
        bundle_path = root / "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl"
        receipt_path.write_bytes(receipt)
        bundle_path.write_bytes(bundle)
        verified = invoke_gh(path, receipt_path, bundle_path, preview["source_sha"])
    bootstrap.require(isinstance(verified, list) and len(verified) == 1)
    result = verified[0].get("verificationResult", {})
    certificate = result.get("signature", {}).get("certificate")
    bootstrap.require(certificate_matches(certificate, preview["source_sha"], preview["signer_workflow_path"], preview["signer_run_id"], preview["signer_run_attempt"]))
    statement = result.get("statement", {})
    bootstrap.require(statement.get("predicateType") == "https://slsa.dev/provenance/v1")
    subjects = statement.get("subject")
    bootstrap.require(isinstance(subjects, list))
    for name, digest in [("SDK_PREVIEW_RECEIPT.json", preview["receipt_sha256"]), (preview["payload_name"], preview["payload_sha256"])]:
        matches = [item for item in subjects if isinstance(item, dict) and item.get("name") == name]
        bootstrap.require(len(matches) == 1 and matches[0].get("digest") == {"sha256": digest})


def verify_run(network, preview, prefix, event):
    run_id = preview[prefix + "_run_id"]
    attempt = preview[prefix + "_run_attempt"]
    path = f"/repos/{bootstrap.REPOSITORY}/actions/runs/{run_id}/attempts/{attempt}"
    run = network.api(path)
    bootstrap.require(run.get("id") == run_id and run.get("run_attempt") == attempt)
    bootstrap.require(run.get("event") == event and run.get("head_sha") == preview["source_sha"])
    bootstrap.require(run.get("head_branch") == (preview["tag"] if prefix == "signer" else "main") and run.get("path") == preview[prefix + "_workflow_path"])
    bootstrap.require(run.get("status") == "completed" and run.get("conclusion") == "success")
    bootstrap.require(run.get("head_repository", {}).get("id") == bootstrap.REPOSITORY_ID)
    bootstrap.require(run.get("head_repository", {}).get("full_name") == bootstrap.REPOSITORY)
    return path


def verify_required_job(network, run_path, name, source):
    jobs = []
    total = None
    for page in range(1, 11):
        response = network.api(f"{run_path}/jobs?per_page=100&page={page}")
        count = response.get("total_count")
        values = response.get("jobs")
        bootstrap.require(bootstrap.number(count) and isinstance(values, list))
        if total is None:
            total = count
        bootstrap.require(total == count and count <= 1_000)
        jobs.extend(values)
        bootstrap.require(len(jobs) <= total)
        if len(jobs) == total:
            break
        bootstrap.require(len(values) == 100)
    bootstrap.require(len(jobs) == total)
    matches = [job for job in jobs if isinstance(job, dict) and job.get("name") == name]
    bootstrap.require(len(matches) == 1)
    job = matches[0]
    bootstrap.require(job.get("status") == "completed" and job.get("conclusion") == "success")
    bootstrap.require(job.get("head_sha") == source)


def verify_staging(pr_number, gh_path, network, temp_root):
    bootstrap.require(bootstrap.number(pr_number))
    repository = network.api(f"/repos/{bootstrap.REPOSITORY}")
    bootstrap.require(repository.get("id") == bootstrap.REPOSITORY_ID and repository.get("full_name") == bootstrap.REPOSITORY and repository.get("default_branch") == "main")
    pr = network.api(f"/repos/{bootstrap.REPOSITORY}/pulls/{pr_number}")
    bootstrap.require(pr.get("number") == pr_number and pr.get("state") == "open")
    head = pr.get("head", {})
    base = pr.get("base", {})
    bootstrap.require(bootstrap.sha40(head.get("sha")) and bootstrap.sha40(base.get("sha")))
    bootstrap.require(base.get("ref") == "main" and base.get("repo", {}).get("id") == bootstrap.REPOSITORY_ID)
    bootstrap.require(bootstrap.number(head.get("repo", {}).get("id")))
    initial = (head["sha"], head["repo"]["id"], base["sha"])
    main = network.api(f"/repos/{bootstrap.REPOSITORY}/commits/main")
    bootstrap.require(main.get("sha") == base["sha"])
    lock = bootstrap.read_github_lock(network, head["sha"])
    preview = lock.get("planner-preview-staged")
    bootstrap.validate_preview(preview, head["sha"])
    ancestry = network.api(f"/repos/{bootstrap.REPOSITORY}/compare/{preview['source_sha']}...{base['sha']}")
    bootstrap.require(ancestry.get("status") in ("identical", "ahead") and ancestry.get("merge_base_commit", {}).get("sha") == preview["source_sha"])
    ci = verify_run(network, preview, "ci", "push")
    qualification = verify_run(network, preview, "qualification", "workflow_dispatch")
    verify_run(network, preview, "signer", "push")
    verify_required_job(network, ci, "Required", preview["source_sha"])
    verify_required_job(network, qualification, "Source qualification", preview["source_sha"])
    tag_sha = bootstrap.verify_annotated_tag(preview, network)
    receipt, bundle, _ = bootstrap.download_verified_release(preview, network)
    verify_attestation(gh_path, receipt, bundle, preview, temp_root)
    current = network.api(f"/repos/{bootstrap.REPOSITORY}/pulls/{pr_number}")
    bootstrap.require(current.get("number") == pr_number and current.get("state") == "open")
    bootstrap.require((current.get("head", {}).get("sha"), current.get("head", {}).get("repo", {}).get("id"), current.get("base", {}).get("sha")) == initial)
    bootstrap.require(current.get("base", {}).get("ref") == "main" and current.get("base", {}).get("repo", {}).get("id") == bootstrap.REPOSITORY_ID)
    bootstrap.require(network.api(f"/repos/{bootstrap.REPOSITORY}/commits/main").get("sha") == base["sha"])
    bootstrap.require(bootstrap.verify_annotated_tag(preview, network) == tag_sha)
    return {"repository": bootstrap.REPOSITORY, "pull_request": pr_number, "head_sha": head["sha"], "base_sha": base["sha"], "source_sha": preview["source_sha"], "receipt_sha256": preview["receipt_sha256"]}


def main():
    bootstrap.require(len(sys.argv) == 4)
    pr_number = int(sys.argv[1])
    gh = str(Path(sys.argv[2]))
    temp_root = Path(sys.argv[3])
    bootstrap.require(temp_root.is_absolute() and temp_root.is_dir() and not temp_root.is_symlink())
    result = verify_staging(pr_number, gh, bootstrap.GitHubTransport(os.environ.get("GH_TOKEN", "")), temp_root)
    sys.stdout.write(json.dumps(result, sort_keys=True, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    try:
        main()
    except (bootstrap.AdmissionError, OSError, ValueError, KeyError, subprocess.SubprocessError):
        raise SystemExit("source preview staging verification rejected") from None
