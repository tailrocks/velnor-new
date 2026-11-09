# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import urllib.request
from email.message import Message

spec = importlib.util.spec_from_file_location("source_preview_bootstrap", sys.argv[1])
bootstrap = importlib.util.module_from_spec(spec)
sys.modules["source_preview_bootstrap"] = bootstrap
spec.loader.exec_module(bootstrap)
SOURCE = "a" * 40
BASE = "b" * 40
HEAD = "c" * 40
PAYLOAD = b"real selected SDK bytes"
DARWIN_PAYLOAD = b"real Darwin SDK bytes"
BUNDLE = b"previously verified receipt bundle bytes"


def sha(value):
    return hashlib.sha256(value).hexdigest()


def record(source=SOURCE):
    value = {"schema": 3, "repository": bootstrap.REPOSITORY, "version": "0.1.75", "source_sha": source, "tag": "sdk-preview-" + source, "release_id": 10, "bundle_sha256": sha(BUNDLE)}
    for role, path, run in [("source", ".github/workflows/product-release-generator.yml", 11), ("draft", ".github/workflows/product-release-generator.yml", 11), ("signer", ".github/workflows/product-release-generator.yml", 11), ("ci", ".github/workflows/ci.yml", 12), ("qualification", ".github/workflows/source-qualification.yml", 13)]:
        value.update({role + "_workflow_path": path, role + "_workflow_ref": f"{bootstrap.REPOSITORY}/{path}@" + ("refs/tags/sdk-preview-" + source if role in ("source", "draft", "signer") else "refs/heads/main"), role + "_workflow_sha": source, role + "_run_id": run, role + "_run_attempt": 1})
    value.update({"candidate_source_artifact_id": 22, "publisher_run_attempt": 3, "publisher_job_id": 24, "signer_run_attempt": 2})
    for prefix, target, payload, producer_id, retained_id, job_id in (("linux", bootstrap.TARGET, PAYLOAD, 21, 23, 26), ("darwin", "aarch64-apple-darwin", DARWIN_PAYLOAD, 27, 28, 29)):
        value.update({f"{prefix}_producer_run_id": 20, f"{prefix}_producer_run_attempt": 1, f"{prefix}_producer_payload_artifact_id": producer_id, f"{prefix}_producer_payload_archive_sha256": "e" * 64, f"{prefix}_candidate_payload_artifact_id": retained_id, f"{prefix}_qualification_job_id": job_id, f"{prefix}_payload_target": target, f"{prefix}_payload_name": "velnor-actions-" + target, f"{prefix}_payload_size": len(payload), f"{prefix}_payload_sha256": sha(payload)})
    receipt = json.dumps(bootstrap.receipt_from_record(value), sort_keys=True, separators=(",", ":")).encode()
    value["receipt_sha256"] = sha(receipt)
    return value, receipt


valid_retry, retry_receipt = record()
bootstrap.validate_preview(valid_retry, HEAD)
assert bootstrap.approved_preview({"runtime-preview": valid_retry}, {"runtime-preview-staged": valid_retry}, "runtime", HEAD) == valid_retry
for candidate_lock, base_lock in (({"runtime-preview": valid_retry}, {"planner-preview-staged": valid_retry}), ({}, {"runtime-preview": valid_retry}), ({"runtime-preview": valid_retry}, {})):
    try:
        bootstrap.approved_preview(candidate_lock, base_lock, "runtime", HEAD)
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("unapproved runtime preview was accepted")
assert json.loads(retry_receipt)["signer"]["run_attempt"] == 2
assert "publisher_run_attempt" not in json.loads(retry_receipt)
for field, value in (
    ("source_run_id", 99), ("source_run_attempt", 2), ("draft_run_attempt", 3),
    ("publisher_run_attempt", 1), ("publisher_run_attempt", 52),
    ("publisher_job_id", 0), ("linux_producer_payload_artifact_id", 0),
    ("candidate_source_artifact_id", 23), ("schema", 1),
    ("unapproved", True),
):
    changed = dict(valid_retry)
    changed[field] = value
    try:
        bootstrap.validate_preview(changed, HEAD)
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("accepted invalid retry custody: " + field)


stage_spec = importlib.util.spec_from_file_location("source_preview_stage_verify", Path(sys.argv[1]).with_name("source_preview_stage_verify.py"))
stage = importlib.util.module_from_spec(stage_spec)
stage_spec.loader.exec_module(stage)


authority = {"repository": bootstrap.REPOSITORY, "path": stage.AUTHORITY_WORKFLOW, "sha": "d" * 40, "ref": bootstrap.REPOSITORY + "/" + stage.AUTHORITY_WORKFLOW + "@" + "d" * 40}
caller_uri = "https://github.com/" + bootstrap.REPOSITORY + "/.github/workflows/product-release-generator.yml@refs/tags/sdk-preview-" + SOURCE
signer_uri = "https://github.com/" + authority["ref"]
certificate = {
    "issuer": "https://token.actions.githubusercontent.com", "subjectAlternativeName": signer_uri,
    "buildSignerURI": signer_uri, "buildSignerDigest": "d" * 40,
    "buildConfigURI": caller_uri, "buildConfigDigest": SOURCE,
    "sourceRepositoryURI": "https://github.com/" + bootstrap.REPOSITORY,
    "sourceRepositoryIdentifier": str(bootstrap.REPOSITORY_ID),
    "sourceRepositoryOwnerURI": "https://github.com/tailrocks",
    "sourceRepositoryOwnerIdentifier": stage.OWNER_ID,
    "sourceRepositoryDigest": SOURCE, "sourceRepositoryRef": "refs/tags/sdk-preview-" + SOURCE,
    "buildTrigger": "push", "runnerEnvironment": "github-hosted",
    "runInvocationURI": "https://github.com/" + bootstrap.REPOSITORY + "/actions/runs/11/attempts/2",
}
assert stage.certificate_matches(certificate, SOURCE, ".github/workflows/product-release-generator.yml", 11, 2, authority)
for field, value in (
    ("subjectAlternativeName", caller_uri), ("buildSignerURI", caller_uri),
    ("buildSignerDigest", SOURCE), ("buildConfigDigest", "d" * 40),
    ("buildConfigURI", signer_uri), ("sourceRepositoryDigest", "d" * 40),
    ("sourceRepositoryRef", "refs/heads/main"),
    ("runInvocationURI", "https://github.com/" + bootstrap.REPOSITORY + "/actions/runs/11/attempts/1"),
):
    changed = dict(certificate)
    changed[field] = value
    assert not stage.certificate_matches(changed, SOURCE, ".github/workflows/product-release-generator.yml", 11, 2, authority), field
for changed in (dict(authority, ref=bootstrap.REPOSITORY + "/" + stage.AUTHORITY_WORKFLOW + "@refs/heads/main"), dict(authority, path=".github/workflows/candidate.yml"), dict(authority, sha=None)):
    try:
        stage.checked_authority(changed)
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("accepted mutable or missing publication authority")
assert stage.staging_authority() == {
    "repository": bootstrap.REPOSITORY,
    "path": ".github/workflows/source-preview-publication.yml",
    "ref": bootstrap.REPOSITORY + "/.github/workflows/source-preview-publication.yml@3a3c641b531a69b7e4142b61aecf536007bb3f2e",
    "sha": "3a3c641b531a69b7e4142b61aecf536007bb3f2e",
}
enabled_authority_sha = stage.PUBLICATION_AUTHORITY_SHA
try:
    stage.PUBLICATION_AUTHORITY_SHA = None
    try:
        stage.staging_authority()
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("staging accepted a missing publication authority pin")
finally:
    stage.PUBLICATION_AUTHORITY_SHA = enabled_authority_sha


class RoleEvidence:
    def __init__(self, failed_role=None, wrong_publisher=False):
        self.failed_role = failed_role
        self.wrong_publisher = wrong_publisher

    def api(self, path):
        if "/actions/jobs/" in path:
            return {"id": 25 if self.wrong_publisher else 24, "run_id": 11, "run_attempt": 3, "head_sha": SOURCE, "name": "Source preview publication / Publish source preview", "status": "completed", "conclusion": "success"}
        attempt = int(path.split("/attempts/")[1].split("/")[0])
        if "/jobs?" in path:
            roles = (("Source preview publication / Admit source preview", 1, 21), ("Source preview publication / Draft source preview", 1, 22), ("Source preview publication / Attest source preview", 2, 23), ("Source preview publication / Publish source preview", 3, 24))
            jobs = [{"id": job_id, "run_id": 11, "run_attempt": attempt, "name": name, "head_sha": SOURCE, "status": "completed", "conclusion": "failure" if name == self.failed_role else "success"} for name, origin, job_id in roles if origin == attempt]
            return {"total_count": len(jobs), "jobs": jobs}
        return {"id": 11, "run_attempt": attempt, "event": "push", "head_sha": SOURCE, "head_branch": "sdk-preview-" + SOURCE, "path": ".github/workflows/product-release-generator.yml", "status": "completed", "conclusion": "failure", "head_repository": {"id": bootstrap.REPOSITORY_ID, "full_name": bootstrap.REPOSITORY}}


stage.verify_publication_jobs(RoleEvidence(), valid_retry)
for failed_role in ("Source preview publication / Admit source preview", "Source preview publication / Draft source preview", "Source preview publication / Attest source preview", "Source preview publication / Publish source preview", None):
    try:
        stage.verify_publication_jobs(RoleEvidence(failed_role, wrong_publisher=failed_role is None), valid_retry)
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("accepted failed or mismatched publication job: " + str(failed_role))


def lock(value, section):
    lines = [f"[{section}]"]
    for key, item in value.items():
        lines.append(f"{key} = {json.dumps(item)}")
    return ("\n".join(lines) + "\n").encode()


class FakeGitHub:
    def __init__(self, value, receipt, approved):
        self.value = value
        self.stable_payload = PAYLOAD
        self.receipt = receipt
        self.approved = approved
        self.asset_calls = 0
        self.asset_urls = []
        self.base_calls = 0
        self.retarget = False
        self.tamper = False
        self.missing = False
        self.tag_mutation = None
        self.tag_reads = 0
        self.replace_after_tag_read = None
        self.event = {"repository": {"id": bootstrap.REPOSITORY_ID, "full_name": bootstrap.REPOSITORY}, "number": 7, "pull_request": {"base": {"sha": BASE, "ref": "main", "repo": {"id": bootstrap.REPOSITORY_ID}}, "head": {"sha": HEAD, "repo": {"id": bootstrap.REPOSITORY_ID}}}}

    def api(self, path):
        if path == f"/repos/{bootstrap.REPOSITORY}":
            return {"id": bootstrap.REPOSITORY_ID, "full_name": bootstrap.REPOSITORY, "default_branch": "main"}
        if "/pulls/" in path:
            self.base_calls += 1
            result = json.loads(json.dumps(self.event["pull_request"]))
            if self.retarget and self.base_calls > 1:
                result["base"]["sha"] = "d" * 40
            return {"number": 7, "state": "open", **result}
        if "/commits/main" in path:
            return {"sha": BASE}
        if "/contents/" in path:
            data = self.approved
            return {"type": "file", "encoding": "base64", "size": len(data), "path": ".velnor/generator.lock", "content": base64.b64encode(data).decode()}
        if "/git/ref/tags/" in path:
            self.tag_reads += 1
            if self.replace_after_tag_read is not None and self.tag_reads == 2:
                self.replace_after_tag_read.unlink()
                self.replace_after_tag_read.write_bytes(b"unapproved replacement")
            moved = (self.tag_mutation == "moved-tag" and self.tag_reads > 2) or (self.tag_mutation == "verify-moved" and self.tag_reads % 2 == 0)
            return {"ref": "refs/tags/" + self.value["tag"], "object": {"type": "commit" if self.tag_mutation == "lightweight-tag" else "tag", "sha": "f" * 40 if moved else "e" * 40}}
        if "/git/tags/" in path:
            return {"sha": path.rsplit("/", 1)[-1], "tag": "wrong" if self.tag_mutation == "wrong-tag-name" else self.value["tag"], "object": {"type": "tag" if self.tag_mutation == "nested-tag" else "commit", "sha": "d" * 40 if self.tag_mutation == "wrong-tag-commit" or path.endswith("f" * 40) else self.value["source_sha"]}}
        if "/releases/" in path:
            source = self.value["source_sha"]
            prefix = f"https://github.com/{bootstrap.REPOSITORY}/releases/download/sdk-preview-{source}/"
            rows = [("SDK_PREVIEW_RECEIPT.json", self.receipt), ("SDK_PREVIEW_RECEIPT.json.sigstore.jsonl", BUNDLE), (self.value["linux_payload_name"], PAYLOAD), (self.value["darwin_payload_name"], DARWIN_PAYLOAD)]
            if self.missing:
                rows.pop()
            return {"id": 10, "tag_name": "sdk-preview-" + source, "target_commitish": source, "draft": False, "prerelease": True, "immutable": True, "assets": [{"name": name, "state": "uploaded", "size": len(data), "digest": "sha256:" + sha(data), "browser_download_url": prefix + name} for name, data in rows]}
        raise AssertionError(path)

    def asset(self, url, maximum):
        self.asset_calls += 1
        self.asset_urls.append(url)
        if "/releases/download/v0.1.74/" in url:
            return self.stable_payload + (b"!" if self.tamper else b"")
        if url.endswith("SDK_PREVIEW_RECEIPT.json"):
            return self.receipt
        if url.endswith("SDK_PREVIEW_RECEIPT.json.sigstore.jsonl"):
            return BUNDLE
        selected = DARWIN_PAYLOAD if url.endswith("velnor-actions-aarch64-apple-darwin") else PAYLOAD
        return selected + (b"!" if self.tamper else b"")


def scenario(base_section, mutation=None):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory).resolve()
        repo = root / "repo"
        repo.mkdir()
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        subprocess.run(["git", "-C", str(repo), "-c", "user.name=Alexey Zhokhov", "-c", "user.email=alexey@zhokhov.com", "commit", "--allow-empty", "-s", "-qm", "fixture", "-m", "Co-authored-by: Codex <codex@openai.com>"], check=True)
        candidate = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
        value, receipt = record()
        candidate_value = dict(value)
        if mutation == "candidate":
            candidate_value, receipt = record("d" * 40)
        if mutation == "same-source":
            candidate_value["source_sha"] = candidate
        dot = repo / ".velnor"
        dot.mkdir()
        (dot / "generator.lock").write_bytes(lock(candidate_value, "planner-preview"))
        approved = lock(value, base_section) if base_section else b"schema = 1\n"
        transport = FakeGitHub(candidate_value, receipt, approved)
        transport.retarget = mutation == "retarget"
        transport.tamper = mutation == "payload"
        transport.missing = mutation == "missing-asset"
        transport.tag_mutation = mutation
        context = {"event": transport.event, "event_name": "pull_request", "repository": bootstrap.REPOSITORY, "candidate": candidate, "ref": "refs/pull/7/merge", "workspace": str(repo), "lock_path": str(dot / "generator.lock"), "runner_temp": str(root)}
        output = root / "velnor" / "bin" / "controller" / "binary"
        fixed_identity = bootstrap.preview_identity(candidate_value)
        if mutation == "wrong-fixed":
            fixed_identity["sha256"] = "f" * 64
        if mutation is None:
            result = bootstrap.authorize_and_acquire(context, transport, fixed_identity)
            assert Path(result) == output and output.read_bytes() == PAYLOAD and os.access(output, os.X_OK)
        else:
            try:
                bootstrap.authorize_and_acquire(context, transport, fixed_identity)
            except (bootstrap.AdmissionError, OSError):
                pass
            else:
                raise AssertionError(f"accepted {mutation}")
            assert not output.exists(), f"{mutation} wrote output"
            if mutation in ("candidate", "same-source", "wrong-fixed") or base_section is None:
                assert transport.asset_calls == 0, f"{mutation} downloaded assets"


for section in ("planner-preview", "planner-preview-staged"):
    scenario(section)
for mutation in ("candidate", "same-source", "retarget", "payload", "missing-asset", "lightweight-tag", "wrong-tag-name", "wrong-tag-commit", "nested-tag", "moved-tag", "wrong-fixed"):
    scenario("planner-preview-staged", mutation)
scenario(None, "absent")


def stable_lock(payload):
    return ("schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"0.1.74\"\ncommit = \"" + "e" * 40 + "\"\n[[generator.binaries]]\ntarget = \"" + bootstrap.TARGET + "\"\nartifact = \"https://github.com/" + bootstrap.REPOSITORY + "/releases/download/v0.1.74/velnor-actions-0.1.74-" + bootstrap.TARGET + "\"\nsha256 = \"" + sha(payload) + "\"\n").encode()


def stable_scenario(base_active, candidate_active, staged=False, tamper=False, existing=False):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory).resolve()
        repo = root / "repo"
        repo.mkdir()
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        subprocess.run(["git", "-C", str(repo), "-c", "user.name=Alexey Zhokhov", "-c", "user.email=alexey@zhokhov.com", "commit", "--allow-empty", "-s", "-qm", "fixture", "-m", "Co-authored-by: Codex <codex@openai.com>"], check=True)
        candidate = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
        value, receipt = record()
        stable = stable_lock(PAYLOAD)
        base = stable + (lock(value, "planner-preview") if base_active else b"") + (lock(value, "planner-preview-staged") if staged else b"")
        dot = repo / ".velnor"
        dot.mkdir()
        (dot / "generator.lock").write_bytes(lock(value, "planner-preview") if candidate_active else b"schema = 1\n")
        transport = FakeGitHub(value, receipt, base)
        transport.tamper = tamper
        context = {"event": transport.event, "event_name": "pull_request", "repository": bootstrap.REPOSITORY, "candidate": candidate, "ref": "refs/pull/7/merge", "workspace": str(repo), "lock_path": str(dot / "generator.lock"), "runner_temp": str(root)}
        output = root / "velnor" / "bin" / "controller" / "binary"
        if existing:
            output.parent.mkdir(parents=True)
            (output.parent / "foreign").write_bytes(b"preserve")
        if (base_active and not candidate_active) or tamper or existing:
            try:
                bootstrap.authorize_and_acquire(context, transport)
            except (bootstrap.AdmissionError, OSError):
                pass
            else:
                raise AssertionError("overwrote existing controller directory" if existing else "accepted tampered stable controller" if tamper else "accepted active preview downgrade")
            assert not output.exists()
            assert transport.asset_calls == (1 if tamper or existing else 0)
            if existing:
                assert (output.parent / "foreign").read_bytes() == b"preserve"
        else:
            result = bootstrap.authorize_and_acquire(context, transport)
            assert Path(result) == output and output.read_bytes() == PAYLOAD
            identity_path = output.parent / "identity.json"
            identity_bytes = identity_path.read_bytes()
            identity = bootstrap.verify_controller(context, transport)
            assert identity["channel"] == ("source-preview" if candidate_active else "stable")
            assert identity["version"] == ("0.1.75" if candidate_active else "0.1.74")
            assert identity["sha256"] == sha(PAYLOAD)
            if candidate_active:
                transport.tag_mutation = "verify-moved"
                transport.tag_reads = 0
                try:
                    bootstrap.verify_controller(context, transport)
                except bootstrap.AdmissionError:
                    pass
                else:
                    raise AssertionError("accepted tag movement during identity verification")
                transport.tag_mutation = None
            if not candidate_active:
                assert transport.asset_calls == 1
                assert transport.asset_urls == ["https://github.com/" + bootstrap.REPOSITORY + "/releases/download/v0.1.74/velnor-actions-0.1.74-" + bootstrap.TARGET]
            for attack in ("missing", "extra", "duplicate", "both-changed", "binary"):
                identity_path.write_bytes(identity_bytes)
                output.write_bytes(PAYLOAD)
                if attack == "missing":
                    identity_path.unlink()
                elif attack == "extra":
                    changed = dict(identity, unapproved=True)
                    identity_path.write_bytes(json.dumps(changed, sort_keys=True, separators=(",", ":")).encode())
                elif attack == "duplicate":
                    identity_path.write_bytes(b'{"channel":"stable",' + identity_bytes[1:])
                elif attack == "both-changed":
                    changed = dict(identity, sha256=sha(b"attacker"))
                    identity_path.write_bytes(json.dumps(changed, sort_keys=True, separators=(",", ":")).encode())
                    output.write_bytes(b"attacker")
                else:
                    output.write_bytes(b"attacker")
                try:
                    bootstrap.verify_controller(context, transport)
                except (bootstrap.AdmissionError, OSError):
                    pass
                else:
                    raise AssertionError("accepted unapproved controller identity: " + attack)


for active, candidate, staged in ((False, False, False), (False, False, True), (True, False, False), (True, True, False), (False, True, True)):
    stable_scenario(active, candidate, staged)
stable_scenario(False, False, tamper=True)
stable_scenario(False, False, existing=True)


redirect = bootstrap.FixedRedirect()
request = urllib.request.Request("https://github.com/tailrocks/velnor-new/releases/download/x", headers={"Authorization": "Bearer canary", "Cookie": "secret=canary"})
allowed = redirect.redirect_request(request, None, 302, "Found", Message(), "https://release-assets.githubusercontent.com/object")
assert allowed is not None
assert not any(key.lower() in ("authorization", "cookie") for key in allowed.headers)
for destination in ("https://foreign.example/asset", "https://api.github.com/asset"):
    try:
        redirect.redirect_request(request, None, 302, "Found", Message(), destination)
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError(f"accepted redirect to {destination}")
plain = urllib.request.Request("https://github.com/tailrocks/velnor-new/releases/download/x")
assert redirect.redirect_request(plain, None, 302, "Found", Message(), "https://release-assets.githubusercontent.com/object") is not None
