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
AUTHORITY_WORKFLOW = ".github/workflows/source-preview-publication.yml"
PUBLICATION_AUTHORITY_SHA = "72701af69f6f9b693e639814fcf479409da3295c"


def checked_authority(authority):
    bootstrap.require(isinstance(authority, dict) and set(authority) == {"repository", "path", "ref", "sha"})
    bootstrap.require(authority["repository"] == bootstrap.REPOSITORY and authority["path"] == AUTHORITY_WORKFLOW)
    bootstrap.require(bootstrap.sha40(authority["sha"]))
    bootstrap.require(authority["ref"] == bootstrap.REPOSITORY + "/" + AUTHORITY_WORKFLOW + "@" + authority["sha"])
    return authority


def staging_authority():
    bootstrap.require(bootstrap.sha40(PUBLICATION_AUTHORITY_SHA))
    return checked_authority({"repository": bootstrap.REPOSITORY, "path": AUTHORITY_WORKFLOW, "ref": bootstrap.REPOSITORY + "/" + AUTHORITY_WORKFLOW + "@" + PUBLICATION_AUTHORITY_SHA, "sha": PUBLICATION_AUTHORITY_SHA})


def verify_gh_binary(path):
    expected = GH_SHA256.get((platform.system(), platform.machine()))
    bootstrap.require(expected is not None)
    data = bootstrap.read_regular(path, MAX_GH_BYTES)
    bootstrap.require(hashlib.sha256(data).hexdigest() in expected)
    bootstrap.require(os.access(path, os.X_OK))


def _invoke_gh(path, receipt_path, bundle_path, source, authority, policy, deadline):
    bootstrap.require(authority["sha"] != source)
    environment = {key: os.environ[key] for key in ("HOME", "LANG", "LC_ALL", "TMPDIR") if key in os.environ}
    environment.update({"PATH": "/usr/bin:/bin", "GH_PROMPT_DISABLED": "1"})
    command = [path, "attestation", "verify", str(receipt_path), "--bundle", str(bundle_path), "--repo", policy["repository"], "--signer-workflow", policy["repository"] + "/" + authority["path"], "--signer-digest", authority["sha"], "--source-digest", source, "--source-ref", policy["source_ref"], "--format", "json"]
    if policy.get("trusted_root") is not None:
        command.extend(("--custom-trusted-root", str(policy["trusted_root"])))
    process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=environment, start_new_session=True)
    output = bytearray()
    deadline = min(deadline, time.monotonic() + 90)
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


def _certificate_matches(certificate, source, workflow, run_id, attempt, authority, policy):
    signer_uri = "https://github.com/" + authority["ref"]
    uri = f"https://github.com/{policy['repository']}/{workflow}@{policy['source_ref']}"
    expected = {
        "issuer": "https://token.actions.githubusercontent.com",
        "subjectAlternativeName": signer_uri,
        "buildSignerURI": signer_uri,
        "buildSignerDigest": authority["sha"],
        "buildConfigURI": uri,
        "buildConfigDigest": source,
        "sourceRepositoryURI": f"https://github.com/{policy['repository']}",
        "sourceRepositoryIdentifier": str(policy["repository_id"]),
        "sourceRepositoryOwnerURI": "https://github.com/" + policy["owner"],
        "sourceRepositoryOwnerIdentifier": str(policy["owner_id"]),
        "sourceRepositoryDigest": source,
        "sourceRepositoryRef": policy["source_ref"],
        "buildTrigger": policy["trigger"],
        "runnerEnvironment": "github-hosted",
        "runInvocationURI": f"https://github.com/{policy['repository']}/actions/runs/{run_id}/attempts/{attempt}",
    }
    return authority["sha"] != source and isinstance(certificate, dict) and all(certificate.get(key) == value for key, value in expected.items())


def _public_policy(source):
    return {"repository": bootstrap.REPOSITORY, "repository_id": bootstrap.REPOSITORY_ID,
            "owner": "tailrocks", "owner_id": OWNER_ID, "source_ref": "refs/tags/sdk-preview-" + source,
            "trigger": "push", "trusted_root": None}


def certificate_matches(certificate, source, workflow, run_id, attempt, expected_authority):
    return _certificate_matches(certificate, source, workflow, run_id, attempt,
                                checked_authority(expected_authority), _public_policy(source))


def verify_attestation(path, receipt, bundle, preview, temp_root, expected_authority, deadline):
    authority = checked_authority(expected_authority)
    _verify_attestation(path, receipt, bundle, preview, temp_root, authority, _public_policy(preview["source_sha"]), deadline)


def _verify_attestation(path, receipt, bundle, preview, temp_root, authority, policy, deadline):
    bootstrap.require(authority["sha"] != preview["source_sha"])
    verify_gh_binary(path)
    with tempfile.TemporaryDirectory(prefix="source-preview-stage-", dir=temp_root) as temporary:
        root = Path(temporary)
        receipt_path = root / "SDK_PREVIEW_RECEIPT.json"
        bundle_path = root / "SDK_PREVIEW_RECEIPT.json.sigstore.jsonl"
        receipt_path.write_bytes(receipt)
        bundle_path.write_bytes(bundle)
        verified = _invoke_gh(path, receipt_path, bundle_path, preview["source_sha"], authority, policy, deadline)
    bootstrap.require(isinstance(verified, list) and len(verified) == 1)
    result = verified[0].get("verificationResult", {})
    certificate = result.get("signature", {}).get("certificate")
    bootstrap.require(_certificate_matches(certificate, preview["source_sha"], preview["signer_workflow_path"], preview["signer_run_id"], preview["signer_run_attempt"], authority, policy))
    statement = result.get("statement", {})
    bootstrap.require(statement.get("predicateType") == "https://slsa.dev/provenance/v1")
    subjects = statement.get("subject")
    bootstrap.require(isinstance(subjects, list) and len(subjects) == 3)
    for name, digest in [("SDK_PREVIEW_RECEIPT.json", preview["receipt_sha256"]), (preview["linux_payload_name"], preview["linux_payload_sha256"]), (preview["darwin_payload_name"], preview["darwin_payload_sha256"])]:
        matches = [item for item in subjects if isinstance(item, dict) and item.get("name") == name]
        bootstrap.require(len(matches) == 1 and matches[0].get("digest") == {"sha256": digest})


def verify_run(network, preview, prefix, event):
    run_id = preview[prefix + "_run_id"]
    attempt = preview[prefix + "_run_attempt"]
    path = f"/repos/{bootstrap.REPOSITORY}/actions/runs/{run_id}/attempts/{attempt}"
    run = network.api(path)
    bootstrap.require(run.get("id") == run_id and run.get("run_attempt") == attempt)
    bootstrap.require(run.get("event") == event and run.get("head_sha") == preview["source_sha"])
    bootstrap.require(run.get("head_branch") == (preview["tag"] if prefix in ("source", "draft", "signer") else "main") and run.get("path") == preview[prefix + "_workflow_path"])
    bootstrap.require(run.get("status") == "completed")
    if prefix in ("ci", "qualification"):
        bootstrap.require(run.get("conclusion") == "success")
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
    bootstrap.require(all(isinstance(job, dict) and bootstrap.number(job.get("id")) for job in jobs))
    bootstrap.require(len({job["id"] for job in jobs}) == len(jobs))
    run_id, attempt = (int(run_path.split("/runs/")[1].split("/")[0]), int(run_path.rsplit("/", 1)[1]))
    bootstrap.require(all(job.get("run_id") == run_id and job.get("run_attempt") == attempt and job.get("head_sha") == source for job in jobs))
    matches = [job for job in jobs if isinstance(job, dict) and job.get("name") == name]
    bootstrap.require(len(matches) == 1)
    job = matches[0]
    bootstrap.require(job.get("status") == "completed" and job.get("conclusion") == "success")
    bootstrap.require(job.get("head_sha") == source)
    return job


def verify_publication_jobs(network, preview):
    for role, name in (("source", "Source preview publication / Admit source preview"), ("draft", "Source preview publication / Draft source preview"), ("signer", "Source preview publication / Attest source preview")):
        role_path = verify_run(network, preview, role, "push")
        verify_required_job(network, role_path, name, preview["source_sha"])
    publisher = dict(preview)
    publisher["signer_run_attempt"] = preview["publisher_run_attempt"]
    publisher_path = verify_run(network, publisher, "signer", "push")
    published_job = verify_required_job(network, publisher_path, "Source preview publication / Publish source preview", preview["source_sha"])
    bootstrap.require(published_job["id"] == preview["publisher_job_id"])
    pinned_job = network.api(f"/repos/{bootstrap.REPOSITORY}/actions/jobs/{preview['publisher_job_id']}")
    bootstrap.require(pinned_job.get("id") == preview["publisher_job_id"] and pinned_job.get("run_id") == preview["signer_run_id"])
    bootstrap.require(pinned_job.get("run_attempt") == preview["publisher_run_attempt"] and pinned_job.get("head_sha") == preview["source_sha"])
    bootstrap.require(pinned_job.get("name") == "Source preview publication / Publish source preview" and pinned_job.get("status") == "completed" and pinned_job.get("conclusion") == "success")


def verify_target_producers(network, preview):
    for target, label in (("linux", "Linux"), ("darwin", "Darwin")):
        run_id = preview[f"{target}_producer_run_id"]
        attempt = preview[f"{target}_producer_run_attempt"]
        path = f"/repos/{bootstrap.REPOSITORY}/actions/runs/{run_id}/attempts/{attempt}"
        run = network.api(path)
        bootstrap.require(run.get("id") == run_id and run.get("run_attempt") == attempt)
        bootstrap.require(run.get("head_sha") == preview["source_sha"] and run.get("head_branch") == "main")
        bootstrap.require(run.get("event") == "push" and run.get("path", "").split("@", 1)[0] == ".github/workflows/product-release-generator.yml")
        bootstrap.require(run.get("status") == "completed" and run.get("conclusion") == "success")
        bootstrap.require(run.get("head_repository", {}).get("id") == bootstrap.REPOSITORY_ID)
        verify_required_job(network, path, f"Build source preview {label}", preview["source_sha"])
        job = verify_required_job(network, path, f"Qualify source preview {label}", preview["source_sha"])
        bootstrap.require(job["id"] == preview[f"{target}_qualification_job_id"])
        artifact_id = preview[f"{target}_producer_payload_artifact_id"]
        artifact = network.api(f"/repos/{bootstrap.REPOSITORY}/actions/artifacts/{artifact_id}")
        bootstrap.require(artifact.get("id") == artifact_id and artifact.get("expired") is False)
        bootstrap.require(artifact.get("name") == f"source-preview-payload-{target}-{attempt}")
        bootstrap.require(artifact.get("digest") == "sha256:" + preview[f"{target}_producer_payload_archive_sha256"])
        bootstrap.require(artifact.get("workflow_run", {}).get("id") == run_id and artifact.get("workflow_run", {}).get("head_sha") == preview["source_sha"])


def verify_preview(preview, gh_path, network, temp_root, expected_authority):
    ci = verify_run(network, preview, "ci", "push")
    qualification = verify_run(network, preview, "qualification", "workflow_dispatch")
    verify_required_job(network, ci, "Required", preview["source_sha"])
    verify_required_job(network, qualification, "Source qualification", preview["source_sha"])
    verify_target_producers(network, preview)
    verify_publication_jobs(network, preview)
    tag_sha = bootstrap.verify_annotated_tag(preview, network)
    receipt, bundle, payloads = bootstrap.download_verified_release(preview, network)
    authority = checked_authority(expected_authority)
    authority_ancestry = network.api(f"/repos/{bootstrap.REPOSITORY}/compare/{authority['sha']}...{preview['source_sha']}")
    bootstrap.require(authority_ancestry.get("status") == "ahead" and authority_ancestry.get("merge_base_commit", {}).get("sha") == authority["sha"])
    verify_attestation(gh_path, receipt, bundle, preview, temp_root, authority, network.deadline)
    bootstrap.require(bootstrap.verify_annotated_tag(preview, network) == tag_sha)
    return receipt, bundle, payloads


def verify_staging(pr_number, gh_path, network, temp_root, role):
    bootstrap.require(role in ("planner", "runtime"))
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
    preview = lock.get(f"{role}-preview-staged")
    bootstrap.validate_preview(preview, head["sha"])
    ancestry = network.api(f"/repos/{bootstrap.REPOSITORY}/compare/{preview['source_sha']}...{base['sha']}")
    bootstrap.require(ancestry.get("status") in ("identical", "ahead") and ancestry.get("merge_base_commit", {}).get("sha") == preview["source_sha"])
    verify_preview(preview, gh_path, network, temp_root, staging_authority())
    current = network.api(f"/repos/{bootstrap.REPOSITORY}/pulls/{pr_number}")
    bootstrap.require(current.get("number") == pr_number and current.get("state") == "open")
    bootstrap.require((current.get("head", {}).get("sha"), current.get("head", {}).get("repo", {}).get("id"), current.get("base", {}).get("sha")) == initial)
    bootstrap.require(current.get("base", {}).get("ref") == "main" and current.get("base", {}).get("repo", {}).get("id") == bootstrap.REPOSITORY_ID)
    bootstrap.require(network.api(f"/repos/{bootstrap.REPOSITORY}/commits/main").get("sha") == base["sha"])
    return {"role": role, "repository": bootstrap.REPOSITORY, "pull_request": pr_number, "head_sha": head["sha"], "base_sha": base["sha"], "source_sha": preview["source_sha"], "receipt_sha256": preview["receipt_sha256"]}


def main():
    bootstrap.require(len(sys.argv) == 5)
    pr_number = int(sys.argv[1])
    gh = str(Path(sys.argv[2]))
    temp_root = Path(sys.argv[3])
    bootstrap.require(temp_root.is_absolute() and temp_root.is_dir() and not temp_root.is_symlink())
    result = verify_staging(pr_number, gh, bootstrap.GitHubTransport(os.environ.get("GH_TOKEN", "")), temp_root, sys.argv[4])
    sys.stdout.write(json.dumps(result, sort_keys=True, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    try:
        main()
    except (bootstrap.AdmissionError, OSError, ValueError, KeyError, subprocess.SubprocessError):
        raise SystemExit("source preview staging verification rejected") from None
