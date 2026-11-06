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
spec.loader.exec_module(bootstrap)
SOURCE = "a" * 40
BASE = "b" * 40
HEAD = "c" * 40
PAYLOAD = b"real selected SDK bytes"
BUNDLE = b"previously verified receipt bundle bytes"


def sha(value):
    return hashlib.sha256(value).hexdigest()


def record(source=SOURCE):
    value = {"schema": 1, "repository": bootstrap.REPOSITORY, "version": "0.1.75", "source_sha": source, "tag": "sdk-preview-" + source, "release_id": 10, "payload_target": bootstrap.TARGET, "payload_name": "velnor-actions-x86_64-unknown-linux-gnu", "payload_size": len(PAYLOAD), "payload_sha256": sha(PAYLOAD), "bundle_sha256": sha(BUNDLE)}
    for role, path, run in [("signer", ".github/workflows/product-release-generator.yml", 11), ("ci", ".github/workflows/ci.yml", 12), ("qualification", ".github/workflows/source-qualification.yml", 13)]:
        value.update({role + "_workflow_path": path, role + "_workflow_ref": f"{bootstrap.REPOSITORY}/{path}@" + ("refs/tags/sdk-preview-" + source if role == "signer" else "refs/heads/main"), role + "_workflow_sha": source, role + "_run_id": run, role + "_run_attempt": 1})
    receipt = json.dumps(bootstrap.receipt_from_record(value), sort_keys=True, separators=(",", ":")).encode()
    value["receipt_sha256"] = sha(receipt)
    return value, receipt


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
            rows = [("SDK_PREVIEW_RECEIPT.json", self.receipt), ("SDK_PREVIEW_RECEIPT.json.sigstore.jsonl", BUNDLE), (self.value["payload_name"], PAYLOAD)]
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
        return PAYLOAD + (b"!" if self.tamper else b"")


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
        if mutation is None:
            result = bootstrap.authorize_and_acquire(context, transport)
            assert Path(result) == output and output.read_bytes() == PAYLOAD and os.access(output, os.X_OK)
        else:
            try:
                bootstrap.authorize_and_acquire(context, transport)
            except (bootstrap.AdmissionError, OSError):
                pass
            else:
                raise AssertionError(f"accepted {mutation}")
            assert not output.exists(), f"{mutation} wrote output"
            if mutation in ("candidate", "same-source") or base_section is None:
                assert transport.asset_calls == 0, f"{mutation} downloaded assets"


for section in ("planner-preview", "planner-preview-staged"):
    scenario(section)
for mutation in ("candidate", "same-source", "retarget", "payload", "missing-asset", "lightweight-tag", "wrong-tag-name", "wrong-tag-commit", "nested-tag", "moved-tag"):
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
