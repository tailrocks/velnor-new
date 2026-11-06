# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import time
import zipfile

source_path = Path(sys.argv[1])
spec = importlib.util.spec_from_file_location("source_gate", source_path)
gate = importlib.util.module_from_spec(spec)
for sibling in ("artifact", "tag"):
    path = source_path.with_name("schema2_generator_source_preview_" + sibling + ".py")
    exec(compile(path.read_bytes(), str(path), "exec"), gate.__dict__)
spec.loader.exec_module(gate)
SOURCE, TAG_SHA, MAIN = "a" * 40, "b" * 40, "c" * 40
PREFIX = "/repos/" + gate.REPOSITORY
CI, QUAL = ".github/workflows/ci.yml", ".github/workflows/source-qualification.yml"
TAG = "sdk-preview-" + SOURCE
TAG_PATH = PREFIX + "/git/ref/tags/" + TAG
OBJECT_PATH = PREFIX + "/git/tags/" + TAG_SHA
NAME = "velnor-actions-x86_64-unknown-linux-gnu"
PAYLOAD = b"\x7fELF\x02\x01\x01" + b"\0" * 11 + b"\x3e\0" + b"external artifact bytes"
archive_bytes = io.BytesIO()
with zipfile.ZipFile(archive_bytes, "w") as archive:
    entry = zipfile.ZipInfo(NAME)
    entry.external_attr = (stat.S_IFREG | 0o500) << 16
    archive.writestr(entry, PAYLOAD)
ARCHIVE = archive_bytes.getvalue()
ARCHIVE_SHA = hashlib.sha256(ARCHIVE).hexdigest()
NOW = gate.checked_time("2026-11-06T00:00:10Z")
NAMES = {10: ["Required verdict"], 20: ["Source qualification ordinary harness",
    "Source qualification keep harness", "Source qualification generation and repository checks",
    "Source qualification"], 90: ["Build source preview", "Qualify source preview"]}
REQUESTS = []


class Response:
    def __init__(self, status, body=b""):
        self.status, self.body = status, io.BytesIO(body)

    def getheader(self, name, default=None):
        return "https://productionresultssa32.blob.core.windows.net/archive?signature=opaque" if name == "Location" else default

    def read(self, size):
        return self.body.read(size)


class HTTPS:
    def __init__(self, host, **_kwargs):
        self.host, self.sock = host, None

    def request(self, method, path, headers):
        REQUESTS.append((self.host, method, path, headers))

    def getresponse(self):
        return Response(302) if self.host == "api.github.com" else Response(200, ARCHIVE)

    def close(self):
        pass


gate.http.client.HTTPSConnection = HTTPS


def run_record(identity, path, event):
    return {"id": identity, "run_attempt": 2, "head_sha": SOURCE, "head_branch": "main",
        "event": event, "status": "completed", "conclusion": "success", "path": path,
        "head_repository": {"full_name": gate.REPOSITORY}}


def responses():
    proof = {"schema": 1, "repository": gate.REPOSITORY, "artifact": "generator",
        "source_sha": SOURCE, "version": "0.1.75", "producer_run_id": 90,
        "producer_run_attempt": 2, "payload_artifact_id": 700,
        "payload_archive_sha256": ARCHIVE_SHA,
        "immutability": {"enabled": True, "checked_at": "2026-11-06T00:00:00Z"}}
    data = {"/users/donbeave": {"login": "donbeave", "id": 139017},
        TAG_PATH: {"ref": "refs/tags/" + TAG, "object": {"type": "tag", "sha": TAG_SHA}},
        OBJECT_PATH: {"sha": TAG_SHA, "tag": TAG, "message": json.dumps(proof, sort_keys=True, separators=(",", ":")),
            "tagger": {"date": "2026-11-06T00:00:05Z"},
            "object": {"type": "commit", "sha": SOURCE, "url": "https://api.github.com/repos/" + gate.REPOSITORY + "/git/commits/" + SOURCE}},
        PREFIX + "/git/ref/heads/main": {"object": {"type": "commit", "sha": MAIN}},
        PREFIX + "/compare/" + SOURCE + "..." + MAIN: {"status": "ahead", "merge_base_commit": {"sha": SOURCE}},
        PREFIX + "/contents/Cargo.toml?ref=" + SOURCE: {"path": "Cargo.toml", "encoding": "base64",
            "content": base64.b64encode(b'[workspace.package]\nversion="0.1.75"\n').decode()},
        PREFIX + "/rulesets/26696396": {"id": 26696396, "target": "tag", "enforcement": "active",
            "bypass_actors": [{"actor_id": 139017, "actor_type": "User", "bypass_mode": "always"}],
            "conditions": {"ref_name": {"include": ["refs/tags/sdk-preview-*"], "exclude": []}}, "rules": [{"type": "creation"}]},
        PREFIX + "/rulesets/24397132": {"id": 24397132, "target": "tag", "enforcement": "active", "bypass_actors": [],
            "conditions": {"ref_name": {"include": ["~ALL"], "exclude": []}}, "rules": [{"type": "update"}, {"type": "deletion"}]},
        PREFIX + "/actions/runs/90/artifacts?per_page=100": {"total_count": 1, "artifacts": [{"id": 700,
            "name": "source-preview-payload-2", "expired": False, "digest": "sha256:" + ARCHIVE_SHA,
            "size_in_bytes": len(ARCHIVE), "workflow_run": {"id": 90, "head_sha": SOURCE}}]},
    }
    for identity, path, event in [(10, CI, "push"), (20, QUAL, "workflow_dispatch"), (90, gate.WORKFLOW, "push")]:
        record = run_record(identity, path, event)
        data[PREFIX + "/actions/workflows/" + path.rsplit("/", 1)[1] + "/runs?head_sha=" + SOURCE + "&event=" + event + "&status=success&per_page=20"] = {"workflow_runs": [record]}
        data[PREFIX + "/actions/runs/" + str(identity)] = record
        data[PREFIX + "/actions/runs/" + str(identity) + "/attempts/2"] = record
        jobs = [{"id": identity * 100 + index, "name": name, "run_id": identity, "head_sha": SOURCE,
            "status": "completed", "conclusion": "success"} for index, name in enumerate(NAMES[identity])]
        data[PREFIX + "/actions/runs/" + str(identity) + "/attempts/2/jobs?per_page=100"] = {"jobs": jobs, "total_count": len(jobs)}
    return data


class GitHubFixture:
    token = "external-fixture-token"

    def __init__(self, values, moved=False):
        self.values, self.moved, self.reads = values, moved, 0
        self.deadline = time.monotonic() + 120

    def get(self, path):
        value = copy.deepcopy(self.values[path])
        if path == TAG_PATH:
            self.reads += 1
            if self.moved and self.reads > 1:
                value["object"]["sha"] = "d" * 40
        return value


def admitted(values, reject=False, moved=False, downloads=0):
    REQUESTS.clear()
    with tempfile.TemporaryDirectory() as temporary:
        os.environ["RUNNER_TEMP"] = str(Path(temporary).resolve())
        try:
            value = gate.admit(GitHubFixture(values, moved), NOW)
        except (ValueError, KeyError, OSError):
            if not reject:
                raise
            assert not (Path(temporary) / NAME).exists(), "rejected source exposed a payload"
            assert len(REQUESTS) == downloads, "invalid authority downloaded an artifact"
            return
        assert not reject, "unqualified SDK tag was admitted"
        assert value["tag"] == TAG and value["ci"]["run_id"] == 10
        assert (Path(temporary) / NAME).read_bytes() == PAYLOAD
        assert REQUESTS[0][3]["Authorization"] == "Bearer external-fixture-token"
        assert "Authorization" not in REQUESTS[1][3]


os.environ.update({"GITHUB_SHA": SOURCE, "GITHUB_REPOSITORY": gate.REPOSITORY,
    "GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/tags/" + TAG,
    "GITHUB_WORKFLOW_REF": gate.REPOSITORY + "/" + gate.WORKFLOW + "@refs/tags/" + TAG,
    "GITHUB_WORKFLOW_SHA": SOURCE, "SDK_PREVIEW_VERSION": "0.1.75",
    "GITHUB_RUN_ID": "30", "GITHUB_RUN_ATTEMPT": "1", "GITHUB_ACTOR_ID": "139017",
    "GITHUB_TRIGGERING_ACTOR": "donbeave"})
admitted(responses())
for key, bad in [("GITHUB_REF", "refs/heads/main"), ("GITHUB_EVENT_NAME", "workflow_dispatch"),
    ("GITHUB_WORKFLOW_SHA", "b" * 40), ("GITHUB_ACTOR_ID", "999"), ("GITHUB_TRIGGERING_ACTOR", "other")]:
    original = os.environ[key]
    os.environ[key] = bad
    admitted(responses(), reject=True)
    os.environ[key] = original
for field, bad in [("source_sha", "f" * 40), ("version", "0.1.74"), ("payload_artifact_id", 701),
    ("payload_archive_sha256", "0" * 64), ("immutability", {"enabled": False, "checked_at": "2026-11-06T00:00:00Z"}),
    ("immutability", {"enabled": True, "checked_at": "2026-11-05T00:00:00Z"}),
    ("immutability", {"enabled": True, "checked_at": "2026-11-06T00:01:00Z"})]:
    values = responses()
    proof = json.loads(values[OBJECT_PATH]["message"])
    proof[field] = bad
    values[OBJECT_PATH]["message"] = json.dumps(proof, sort_keys=True, separators=(",", ":"))
    admitted(values, reject=True)
for record_path, field, bad in [(TAG_PATH, "object", {"type": "commit", "sha": SOURCE}),
    (OBJECT_PATH, "object", {"type": "tag", "sha": TAG_SHA}),
    (OBJECT_PATH, "tag", "preview-" + SOURCE),
    (PREFIX + "/rulesets/26696396", "enforcement", "disabled"),
    (PREFIX + "/rulesets/24397132", "rules", [{"type": "update"}]),
    (PREFIX + "/actions/runs/90", "run_attempt", 3)]:
    values = responses()
    values[record_path][field] = bad
    admitted(values, reject=True)
for path in [key for key in responses() if "/jobs?" in key]:
    for mode in ("failed", "missing", "duplicate_name", "duplicate_id", "truncated", "wrong_source"):
        values = responses()
        entry = values[path]
        if mode == "failed":
            entry["jobs"][0]["conclusion"] = "skipped"
        elif mode == "missing":
            entry["jobs"].pop(0)
        elif mode == "duplicate_name":
            entry["jobs"].append(dict(entry["jobs"][0], id=99999))
        elif mode == "duplicate_id":
            entry["jobs"].append(dict(entry["jobs"][0], name="unrelated"))
        elif mode == "wrong_source":
            entry["jobs"][0]["head_sha"] = "d" * 40
        entry["total_count"] = len(entry["jobs"]) + (100 if mode == "truncated" else 0)
        admitted(values, reject=True)
admitted(responses(), reject=True, moved=True, downloads=2)
values = responses()
proof = json.loads(values[OBJECT_PATH]["message"])
proof["immutability"]["checked_at"] = "2026-11-05T00:00:00Z"
values[OBJECT_PATH]["message"] = json.dumps(proof, sort_keys=True, separators=(",", ":"))
values[OBJECT_PATH]["tagger"]["date"] = "2026-11-05T00:00:05Z"
admitted(values, reject=True)
