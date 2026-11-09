# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
from email.message import Message
from unittest.mock import patch
from urllib.parse import urlsplit

directory = Path(sys.argv[1])
for name in ("source_preview_bootstrap", "source_preview_stage_verify", "source_preview_runtime"):
    spec = importlib.util.spec_from_file_location(name, directory / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
bootstrap = sys.modules["source_preview_bootstrap"]
runtime = sys.modules["source_preview_runtime"]
C, P, B = "c" * 40, "b" * 40, "a" * 40
PREFIX = "/repos/tailrocks/velnor-new"

for system, machine, expected in [("Linux", "x86_64", "linux"), ("Darwin", "arm64", "darwin")]:
    with patch.object(runtime.platform, "system", return_value=system), patch.object(runtime.platform, "machine", return_value=machine):
        assert runtime.host_target() == expected
with patch.object(runtime.platform, "system", return_value="Linux"), patch.object(runtime.platform, "machine", return_value="arm64"):
    try:
        runtime.host_target()
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("unsupported host selected preview bytes")


def lock(record=None, section="runtime-preview"):
    if record is None:
        return b"[generator]\nbinary = \"velnor-actions\"\n"
    return ("[" + section + "]\n" + "".join(f"{key} = {json.dumps(value)}\n" for key, value in record.items())).encode()


def record():
    result = {"schema": 3, "repository": bootstrap.REPOSITORY, "version": "0.1.75", "source_sha": B, "tag": "sdk-preview-" + B, "release_id": 10, "bundle_sha256": "d" * 64, "candidate_source_artifact_id": 22, "publisher_run_attempt": 3, "publisher_job_id": 24}
    for role, path, run in [("source", ".github/workflows/product-release-generator.yml", 11), ("draft", ".github/workflows/product-release-generator.yml", 11), ("signer", ".github/workflows/product-release-generator.yml", 11), ("ci", ".github/workflows/ci.yml", 12), ("qualification", ".github/workflows/source-qualification.yml", 13)]:
        ref = "refs/tags/sdk-preview-" + B if role in ("source", "draft", "signer") else "refs/heads/main"
        result.update({role + "_workflow_path": path, role + "_workflow_ref": f"{bootstrap.REPOSITORY}/{path}@{ref}", role + "_workflow_sha": B, role + "_run_id": run, role + "_run_attempt": 2 if role == "signer" else 1})
    for role, target, original, retained, job in [("linux", bootstrap.TARGET, 21, 23, 26), ("darwin", "aarch64-apple-darwin", 27, 28, 29)]:
        result.update({f"{role}_producer_run_id": 20, f"{role}_producer_run_attempt": 1, f"{role}_producer_payload_artifact_id": original, f"{role}_producer_payload_archive_sha256": "e" * 64, f"{role}_candidate_payload_artifact_id": retained, f"{role}_qualification_job_id": job, f"{role}_payload_target": target, f"{role}_payload_name": "velnor-actions-" + target, f"{role}_payload_size": 5, f"{role}_payload_sha256": "f" * 64})
    result["receipt_sha256"] = hashlib.sha256(json.dumps(bootstrap.receipt_from_record(result), sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return result


def blob(data):
    return {"type": "file", "encoding": "base64", "path": ".velnor/generator.lock", "size": len(data), "content": base64.b64encode(data).decode()}


class Response(io.BytesIO):
    def __init__(self, data):
        super().__init__(data)
        self.headers = Message()
        self.headers["Content-Length"] = str(len(data))


class Opener:
    def __init__(self, records):
        self.records = records
        self.requests = []

    def open(self, request, timeout):
        url = urlsplit(request.full_url)
        assert url.scheme == "https" and url.hostname == "api.github.com"
        assert request.get_header("Authorization") == "Bearer fixture_token"
        assert timeout > 0
        path = url.path + ("?" + url.query if url.query else "")
        self.requests.append(path)
        assert path in self.records, "unexpected request: " + path
        return Response(json.dumps(self.records[path]).encode())


def records(current=None, parent=None):
    return {
        PREFIX: {"id": 1390620900, "full_name": "tailrocks/velnor-new", "default_branch": "main"},
        PREFIX + "/commits/main": {"sha": C},
        PREFIX + "/commits/" + C: {"sha": C, "parents": [{"sha": P}]},
        PREFIX + "/actions/runs/123/attempts/2": {"id": 123, "run_attempt": 2, "head_sha": C, "head_branch": "main", "event": "push", "path": ".github/workflows/product-release-images.yml", "head_repository": {"id": 1390620900, "full_name": "tailrocks/velnor-new"}, "status": "in_progress", "conclusion": None},
        PREFIX + "/contents/.velnor/generator.lock?ref=" + C: blob(current or lock()),
        PREFIX + "/contents/.velnor/generator.lock?ref=" + P: blob(parent or lock()),
    }


def network(data):
    value = bootstrap.GitHubTransport("fixture_token")
    value.opener = Opener(data)
    return value


control = network(records())
control.opener.records[PREFIX + "/commits/" + C]["parents"].append({"sha": B})
assert runtime.public_binding(control, C, 123, 2) == P
assert bootstrap.read_github_lock(control, C) == {"generator": {"binary": "velnor-actions"}}
assert bootstrap.read_github_lock(control, P) == {"generator": {"binary": "velnor-actions"}}
assert control.opener.requests[-2:] == [PREFIX + "/contents/.velnor/generator.lock?ref=" + C, PREFIX + "/contents/.velnor/generator.lock?ref=" + P]


class AssetOpener:
    def open(self, request, timeout):
        assert urlsplit(request.full_url).hostname == "github.com"
        assert request.get_header("Authorization") is None
        assert timeout > 0
        return Response(b"asset")


asset_client = network(records())
asset_client.opener = AssetOpener()
assert asset_client.asset("https://github.com/tailrocks/velnor-new/releases/download/example/asset", 10) == b"asset"


def rejected(data):
    client = network(data)
    with tempfile.TemporaryDirectory() as temp:
        try:
            runtime.verify_runtime(client, C, 123, 2, "/usr/bin/gh", temp)
        except (bootstrap.AdmissionError, KeyError, ValueError):
            pass
        else:
            raise AssertionError("runtime accepted untrusted public identity or preview")
        assert list(Path(temp).iterdir()) == [], "runtime left an output after rejection"
    return client


rejected(records())
for path, key, value in [
    (PREFIX, "id", 1), (PREFIX, "full_name", "other/repo"), (PREFIX, "default_branch", "other"),
    (PREFIX + "/commits/main", "sha", B), (PREFIX + "/commits/" + C, "sha", B),
    (PREFIX + "/commits/" + C, "parents", []), (PREFIX + "/commits/" + C, "parents", [{"sha": "bad"}]),
    (PREFIX + "/actions/runs/123/attempts/2", "event", "workflow_dispatch"),
    (PREFIX + "/actions/runs/123/attempts/2", "id", 124),
    (PREFIX + "/actions/runs/123/attempts/2", "run_attempt", 1),
    (PREFIX + "/actions/runs/123/attempts/2", "path", ".github/workflows/other.yml"),
    (PREFIX + "/actions/runs/123/attempts/2", "head_branch", "other"),
    (PREFIX + "/actions/runs/123/attempts/2", "head_sha", B),
    (PREFIX + "/actions/runs/123/attempts/2", "status", "queued"),
    (PREFIX + "/actions/runs/123/attempts/2", "conclusion", "success"),
    (PREFIX + "/actions/runs/123/attempts/2", "head_repository", {"id": 1, "full_name": "tailrocks/velnor-new"}),
    (PREFIX + "/actions/runs/123/attempts/2", "head_repository", {"id": 1390620900, "full_name": "other/repo"}),
]:
    data = records()
    data[path][key] = value
    try:
        runtime.public_binding(network(data), C, 123, 2)
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("public binding accepted invalid " + path + " field " + key)

for commit, key, value in [(C, "content", "***"), (P, "path", "other")]:
    data = records()
    data[PREFIX + "/contents/.velnor/generator.lock?ref=" + commit][key] = value
    try:
        bootstrap.read_github_lock(network(data), commit)
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("lock reader accepted invalid " + key)

preview = record()
bootstrap.validate_preview(preview, C)
client = network(records(lock(preview), lock(preview, "runtime-preview-staged")))
candidate = bootstrap.read_github_lock(client, C)
base = bootstrap.read_github_lock(client, P)
assert bootstrap.approved_preview(candidate, base, "runtime", C) == preview
assert client.opener.requests[-2:] == [PREFIX + "/contents/.velnor/generator.lock?ref=" + C, PREFIX + "/contents/.velnor/generator.lock?ref=" + P]
other = copy.deepcopy(preview)
other["release_id"] += 1
for current, parent in [(lock(preview), lock(preview, "planner-preview-staged")), (lock(preview), lock(other, "runtime-preview-staged")), (lock(), lock(preview, "runtime-preview"))]:
    client = network(records(current, parent))
    try:
        bootstrap.approved_preview(bootstrap.read_github_lock(client, C), bootstrap.read_github_lock(client, P), "runtime", C)
    except bootstrap.AdmissionError:
        pass
    else:
        raise AssertionError("runtime preview accepted unapproved parent selection")
later = network(records())
assert runtime.public_binding(later, C, 123, 2) == P
later.opener.records[PREFIX + "/commits/main"]["sha"] = B
try:
    runtime.public_binding(later, C, 123, 2)
except bootstrap.AdmissionError:
    pass
else:
    raise AssertionError("changed current main was accepted")
