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
for sibling in ("artifact", "tag", "authority"):
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
DARWIN_NAME = "velnor-actions-aarch64-apple-darwin"
DARWIN_PAYLOAD = bytes.fromhex("cffaedfe0c0000010000000002000000") + bytes(16) + b"darwin bytes"
darwin_archive = io.BytesIO()
with zipfile.ZipFile(darwin_archive, "w") as archive:
    entry = zipfile.ZipInfo(DARWIN_NAME)
    entry.external_attr = (stat.S_IFREG | 0o500) << 16
    archive.writestr(entry, DARWIN_PAYLOAD)
DARWIN_ARCHIVE = darwin_archive.getvalue()
DARWIN_SHA = hashlib.sha256(DARWIN_ARCHIVE).hexdigest()
NAMES = {10: ["Required verdict"], 20: ["Source qualification ordinary harness",
    "Source qualification keep harness", "Source qualification generation and repository checks",
    "Source qualification"], 90: ["Build source preview Linux", "Build source preview Darwin", "Qualify source preview Linux", "Qualify source preview Darwin"]}
REQUESTS = []


class Response:
    def __init__(self, status, body=b"", location="archive"):
        self.status, self.body, self.location = status, io.BytesIO(body), location

    def getheader(self, name, default=None):
        return "https://productionresultssa32.blob.core.windows.net/" + self.location + "?signature=opaque" if name == "Location" else default

    def read(self, size):
        return self.body.read(size)


class HTTPS:
    def __init__(self, host, **_kwargs):
        self.host, self.sock, self.path = host, None, ""

    def request(self, method, path, headers):
        self.path = path
        REQUESTS.append((self.host, method, path, headers))

    def getresponse(self):
        if self.host == "api.github.com":
            return Response(302, location="darwin" if "/701/" in self.path else "linux")
        return Response(200, DARWIN_ARCHIVE if self.path.startswith("/darwin") else ARCHIVE)

    def close(self):
        pass


gate.http.client.HTTPSConnection = HTTPS


def run_record(identity, path, event):
    return {"id": identity, "run_attempt": 2, "head_sha": SOURCE, "head_branch": "main",
        "event": event, "status": "completed", "conclusion": "success", "path": path,
        "head_repository": {"full_name": gate.REPOSITORY, "id": gate.REPOSITORY_ID}}


def responses():
    proof = {"schema": 2, "repository": gate.REPOSITORY, "artifact": "generator",
        "source_sha": SOURCE, "version": "0.1.75",
        "targets": {"linux": {"producer_run_id": 90, "producer_run_attempt": 2, "payload_artifact_id": 700, "payload_archive_sha256": ARCHIVE_SHA, "qualification_job_id": 9002},
                    "darwin": {"producer_run_id": 90, "producer_run_attempt": 2, "payload_artifact_id": 701, "payload_archive_sha256": DARWIN_SHA, "qualification_job_id": 9003}},
        "immutability": {"enabled": True, "checked_at": "2026-11-06T00:00:00Z"}}
    data = {PREFIX: {"id": gate.REPOSITORY_ID, "full_name": gate.REPOSITORY, "default_branch": "main", "owner": {"id": gate.REPOSITORY_OWNER_ID}},
        "/users/donbeave": {"login": "donbeave", "id": 139017},
        PREFIX + "/actions/runs/30": {"id": 30, "run_attempt": 1, "head_sha": SOURCE,
            "head_branch": TAG, "event": "push", "path": gate.WORKFLOW,
            "head_repository": {"full_name": gate.REPOSITORY, "id": gate.REPOSITORY_ID}, "created_at": "2026-11-06T00:00:10Z"},
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
        PREFIX + "/actions/runs/90/artifacts?per_page=100": {"total_count": 2, "artifacts": [{"id": 700,
            "name": "source-preview-payload-linux-2", "expired": False, "digest": "sha256:" + ARCHIVE_SHA,
            "size_in_bytes": len(ARCHIVE), "workflow_run": {"id": 90, "head_sha": SOURCE}}, {"id": 701,
            "name": "source-preview-payload-darwin-2", "expired": False, "digest": "sha256:" + DARWIN_SHA,
            "size_in_bytes": len(DARWIN_ARCHIVE), "workflow_run": {"id": 90, "head_sha": SOURCE}}]},
    }
    for identity, path, event in [(10, CI, "push"), (20, QUAL, "workflow_dispatch"), (90, gate.WORKFLOW, "push")]:
        record = run_record(identity, path, event)
        data[PREFIX + "/actions/workflows/" + path.rsplit("/", 1)[1] + "/runs?head_sha=" + SOURCE + "&event=" + event + "&status=success&per_page=20"] = {"workflow_runs": [record]}
        data[PREFIX + "/actions/runs/" + str(identity)] = record
        data[PREFIX + "/actions/runs/" + str(identity) + "/attempts/2"] = record
        jobs = [{"id": identity * 100 + index, "name": name, "run_id": identity, "run_attempt": 2, "head_sha": SOURCE,
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
            value = gate.admit(GitHubFixture(values, moved))
        except (ValueError, KeyError, OSError):
            if not reject:
                raise
            assert not (Path(temporary) / NAME).exists(), "rejected source exposed a payload"
            assert len(REQUESTS) == downloads, "invalid authority downloaded an artifact"
            return
        assert not reject, "unqualified SDK tag was admitted"
        assert value["tag"] == TAG and value["ci"]["run_id"] == 10
        assert (Path(temporary) / NAME).read_bytes() == PAYLOAD
        assert (Path(temporary) / DARWIN_NAME).read_bytes() == DARWIN_PAYLOAD
        assert REQUESTS[0][3]["Authorization"] == "Bearer external-fixture-token"
        assert "Authorization" not in REQUESTS[1][3]


os.environ.update({"GITHUB_SHA": SOURCE, "GITHUB_REPOSITORY": gate.REPOSITORY,
    "GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/tags/" + TAG,
    "GITHUB_WORKFLOW_REF": gate.REPOSITORY + "/" + gate.WORKFLOW + "@refs/tags/" + TAG,
    "GITHUB_WORKFLOW_SHA": SOURCE, "AUTHORITY_REPOSITORY": gate.REPOSITORY,
    "AUTHORITY_PATH": ".github/workflows/source-preview-publication.yml", "AUTHORITY_SHA": "d" * 40,
    "AUTHORITY_REF": gate.REPOSITORY + "/.github/workflows/source-preview-publication.yml@" + "d" * 40,
    "GITHUB_RUN_ID": "30", "GITHUB_RUN_ATTEMPT": "1", "GITHUB_ACTOR_ID": "139017",
    "GITHUB_TRIGGERING_ACTOR": "donbeave"})
admitted(responses())
for key, bad in (("AUTHORITY_REPOSITORY", "other/repository"), ("AUTHORITY_PATH", ".github/workflows/other.yml"),
                 ("AUTHORITY_REF", "tailrocks/velnor-new/.github/workflows/source-preview-publication.yml@main"),
                 ("AUTHORITY_SHA", "D" * 40), ("AUTHORITY_SHA", "")):
    original = os.environ[key]
    os.environ[key] = bad
    admitted(responses(), reject=True)
    os.environ[key] = original
original = os.environ.pop("AUTHORITY_SHA")
admitted(responses(), reject=True)
os.environ["AUTHORITY_SHA"] = original
for key, bad in [("GITHUB_REF", "refs/heads/main"), ("GITHUB_EVENT_NAME", "workflow_dispatch"),
    ("GITHUB_WORKFLOW_SHA", "b" * 40), ("GITHUB_ACTOR_ID", "999"), ("GITHUB_TRIGGERING_ACTOR", "other")]:
    original = os.environ[key]
    os.environ[key] = bad
    admitted(responses(), reject=True)
    os.environ[key] = original
for field, bad in [("source_sha", "f" * 40), ("version", "0.1.74"), ("immutability", {"enabled": False, "checked_at": "2026-11-06T00:00:00Z"}),
    ("immutability", {"enabled": True, "checked_at": "2026-11-05T00:00:00Z"}),
    ("immutability", {"enabled": True, "checked_at": "2026-11-06T00:01:00Z"})]:
    values = responses()
    proof = json.loads(values[OBJECT_PATH]["message"])
    proof[field] = bad
    values[OBJECT_PATH]["message"] = json.dumps(proof, sort_keys=True, separators=(",", ":"))
    admitted(values, reject=True)
for target in ("linux", "darwin"):
    for field, bad in (("payload_artifact_id", 999), ("payload_archive_sha256", "0" * 64), ("qualification_job_id", 999)):
        values = responses()
        proof = json.loads(values[OBJECT_PATH]["message"])
        proof["targets"][target][field] = bad
        values[OBJECT_PATH]["message"] = json.dumps(proof, sort_keys=True, separators=(",", ":"))
        admitted(values, reject=True)
for field, bad in (("id", 1), ("owner", {"id": 1})):
    values = responses()
    values[PREFIX][field] = bad
    admitted(values, reject=True)
for path in (PREFIX + "/actions/runs/90/attempts/2", PREFIX + "/actions/runs/30"):
    values = responses()
    values[path]["head_repository"]["id"] = 1
    admitted(values, reject=True)
for record_path, field, bad in [(TAG_PATH, "object", {"type": "commit", "sha": SOURCE}),
    (OBJECT_PATH, "object", {"type": "tag", "sha": TAG_SHA}),
    (OBJECT_PATH, "tag", "preview-" + SOURCE),
    (PREFIX + "/rulesets/26696396", "enforcement", "disabled"),
    (PREFIX + "/rulesets/24397132", "rules", [{"type": "update"}]),
    (PREFIX + "/actions/runs/90/attempts/2", "conclusion", "failure"),
    (PREFIX + "/actions/runs/30", "head_sha", "f" * 40)]:
    values = responses()
    values[record_path][field] = bad
    admitted(values, reject=True)
values = responses()
values[PREFIX + "/actions/runs/90"] = dict(values[PREFIX + "/actions/runs/90"], run_attempt=3)
inventory = values[PREFIX + "/actions/runs/90/artifacts?per_page=100"]
inventory["artifacts"].append(dict(inventory["artifacts"][0], id=702,
    name="source-preview-payload-linux-3", digest="sha256:" + "f" * 64))
inventory["total_count"] = 3
admitted(values)
values = responses()
values[PREFIX + "/actions/runs/30"]["run_attempt"] = 2
os.environ["GITHUB_RUN_ATTEMPT"] = "2"
admitted(values)
os.environ["GITHUB_RUN_ATTEMPT"] = "1"
values = responses()
values[PREFIX + "/actions/runs/90/artifacts?per_page=100"]["artifacts"][0]["expired"] = True
admitted(values, reject=True)


publication_spec = importlib.util.spec_from_file_location("source_preview_publication", source_path.with_name("schema2_generator_source_preview_publication.py"))
publication = importlib.util.module_from_spec(publication_spec)
publication_spec.loader.exec_module(publication)
sys.modules["source_preview_publication"] = publication


def artifact_zip(name, contents):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        entry = zipfile.ZipInfo(name)
        entry.external_attr = (stat.S_IFREG | 0o400) << 16
        archive.writestr(entry, contents)
    return output.getvalue()


proof = json.loads(responses()[OBJECT_PATH]["message"])
targets = {target: {"producer": {key: item[key] for key in ("producer_run_id", "producer_run_attempt", "payload_artifact_id", "payload_archive_sha256")}, "qualification_job_id": item["qualification_job_id"]} for target, item in proof["targets"].items()}
source_tuple = {"workflow_path": gate.WORKFLOW, "workflow_ref": gate.REPOSITORY + "/" + gate.WORKFLOW + "@refs/tags/" + TAG, "workflow_sha": SOURCE, "run_id": 30, "run_attempt": 1}
def run_tuple(identity, path):
    return {"workflow_path": path, "workflow_ref": gate.REPOSITORY + "/" + path + "@refs/heads/main", "workflow_sha": SOURCE, "run_id": identity, "run_attempt": 2}
original = {"schema": 3, "repository": gate.REPOSITORY, "version": "0.1.75", "source_sha": SOURCE, "tag": TAG,
    "source": source_tuple, "targets": targets, "ci": run_tuple(10, CI), "qualification": run_tuple(20, QUAL)}
candidate = {"source_artifact_id": 801, "linux_payload_artifact_id": 802, "darwin_payload_artifact_id": 803}
archives = {801: artifact_zip("source-preview-metadata.json", publication.canonical(original)), 802: artifact_zip(NAME, PAYLOAD), 803: artifact_zip(DARWIN_NAME, DARWIN_PAYLOAD)}
artifacts = {identity: {"id": identity, "name": name, "size_in_bytes": len(archives[identity]), "digest": "sha256:" + hashlib.sha256(archives[identity]).hexdigest(),
    "expired": False, "workflow_run": {"id": 30, "head_sha": SOURCE}} for identity, name in [(801, "source-preview-source-1"), (802, "source-preview-qualified-payload-linux-1"), (803, "source-preview-qualified-payload-darwin-1")]}


class RecoveryHTTP:
    release = {"id": 77, "tag_name": TAG, "body": publication.candidate_marker(original, candidate), "draft": True, "prerelease": True}
    def __init__(self, host, **kwargs):
        self.host, self.sock, self.path = host, None, None
    def request(self, method, path, body=None, headers=None):
        self.path = path
        if self.host == "api.github.com":
            assert headers["Authorization"] == "Bearer fixture-token"
        else:
            assert "Authorization" not in headers
    def getresponse(self):
        if self.host != "api.github.com":
            return Response(200, archives[int(self.path[1:])])
        if "/releases/tags/" in self.path:
            return Response(200, publication.canonical(self.release))
        if "/actions/artifacts/" in self.path:
            identity = int(self.path.split("/actions/artifacts/")[1].split("/")[0])
            if self.path.endswith("/zip"):
                result = Response(302)
                result.getheader = lambda name, default=None: "https://productionresultssa32.blob.core.windows.net/" + str(identity)
                return result
            return Response(200, publication.canonical(artifacts[identity]))
        if "/attempts/1/jobs?" in self.path:
            return Response(200, publication.canonical({"total_count": 1, "jobs": [{"id": 3001, "run_id": 30, "head_sha": SOURCE, "name": "Source preview publication / Admit source preview", "status": "completed", "conclusion": "success"}]}))
        if "/attempts/1" in self.path:
            return Response(200, publication.canonical({"id": 30, "run_attempt": 1, "head_sha": SOURCE, "head_branch": TAG, "event": "push", "path": gate.WORKFLOW, "head_repository": {"full_name": gate.REPOSITORY, "id": gate.REPOSITORY_ID}, "conclusion": "failure"}))
        raise AssertionError(self.path)
    def close(self):
        pass


os.environ["GH_TOKEN"] = "fixture-token"
os.environ["GITHUB_RUN_ATTEMPT"] = "2"
gate.http.client.HTTPSConnection = RecoveryHTTP
values = responses()
values[PREFIX + "/actions/runs/30"]["run_attempt"] = 2
recovered, selected, recovered_payload = publication.load_candidate(publication.GitHub(), original, json.loads(RecoveryHTTP.release["body"].split(":", 1)[1]))
assert recovered == original and selected == candidate and recovered_payload == {"linux": PAYLOAD, "darwin": DARWIN_PAYLOAD}
artifacts[801]["expired"] = True
try:
    publication.load_candidate(publication.GitHub(), original, json.loads(RecoveryHTTP.release["body"].split(":", 1)[1]))
except ValueError:
    pass
else:
    raise AssertionError("expired original source artifact was accepted")
artifacts[801]["expired"] = False
os.environ["GITHUB_RUN_ATTEMPT"] = "1"
gate.http.client.HTTPSConnection = HTTPS
values = responses()
values[PREFIX + "/actions/runs/30"]["created_at"] = "2026-11-06T00:20:00Z"
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
admitted(responses(), reject=True, moved=True, downloads=4)
values = responses()
proof = json.loads(values[OBJECT_PATH]["message"])
proof["immutability"]["checked_at"] = "2026-11-05T00:00:00Z"
values[OBJECT_PATH]["message"] = json.dumps(proof, sort_keys=True, separators=(",", ":"))
values[OBJECT_PATH]["tagger"]["date"] = "2026-11-05T00:00:05Z"
admitted(values, reject=True)
