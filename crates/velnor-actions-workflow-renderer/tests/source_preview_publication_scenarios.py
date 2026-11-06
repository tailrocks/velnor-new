# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import stat
import zipfile

spec = importlib.util.spec_from_file_location("preview_publication", sys.argv[1])
publisher = importlib.util.module_from_spec(spec)
authority_path = Path(sys.argv[1]).with_name("schema2_generator_source_preview_authority.py")
exec(compile(authority_path.read_bytes(), str(authority_path), "exec"), publisher.__dict__)
spec.loader.exec_module(publisher)
source = "a" * 40
signer_path = publisher.WORKFLOW

def run(path, run_id):
    return {"workflow_path": path, "workflow_ref": publisher.REPOSITORY + "/" + path + "@refs/heads/main", "workflow_sha": source, "run_id": run_id, "run_attempt": 1}

tag_ref = "refs/tags/sdk-preview-" + source
metadata = {"schema": 2, "repository": publisher.REPOSITORY, "version": "0.1.75", "source_sha": source, "tag": "sdk-preview-" + source, "source": run(signer_path, 30), "ci": run(".github/workflows/ci.yml", 10), "qualification": run(".github/workflows/source-qualification.yml", 20), "producer": {"producer_run_id": 90, "producer_run_attempt": 2, "payload_artifact_id": 700, "payload_archive_sha256": "b" * 64}}
metadata["source"]["workflow_ref"] = publisher.REPOSITORY + "/" + publisher.WORKFLOW + "@" + tag_ref
candidate = {"source_artifact_id": 100, "payload_artifact_id": 101}
PAYLOAD_BYTES = b"\x7fELF\x02\x01\x01" + b"\0" * 11 + b"\x3e\0" + b"qualified-payload"


def make_archive(name, data):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        entry = zipfile.ZipInfo(name)
        entry.external_attr = (stat.S_IFREG | 0o400) << 16
        archive.writestr(entry, data)
    return output.getvalue()


archives = {100: make_archive("source-preview-metadata.json", publisher.canonical(metadata)), 101: make_archive(publisher.PAYLOAD, PAYLOAD_BYTES)}
os.environ.update({"GITHUB_SHA": source, "GITHUB_REPOSITORY": publisher.REPOSITORY, "GITHUB_EVENT_NAME": "push", "GITHUB_REF": tag_ref, "GITHUB_WORKFLOW_REF": metadata["source"]["workflow_ref"], "GITHUB_WORKFLOW_SHA": source, "GITHUB_RUN_ID": "30", "GITHUB_RUN_ATTEMPT": "1", "PREVIEW_MODE": "draft", "GH_TOKEN": "fixture-token", "SOURCE_ARTIFACT_ID": "100", "PAYLOAD_ARTIFACT_ID": "101"})
os.environ.update({"AUTHORITY_REPOSITORY": publisher.REPOSITORY, "AUTHORITY_PATH": ".github/workflows/source-preview-publication.yml", "AUTHORITY_SHA": "d" * 40, "AUTHORITY_REF": publisher.REPOSITORY + "/.github/workflows/source-preview-publication.yml@" + "d" * 40})

class Response:
    def __init__(self, status, body):
        self.status, self.body = status, publisher.canonical(body)
    def read(self, limit):
        result, self.body = self.body[:limit], self.body[limit:]
        return result
    def getheader(self, name, default=None):
        return getattr(self, "location", default)

class ExternalNetwork:
    release = None
    writes = 0
    redirect = False
    main_sha = source
    ancestor = True
    tag_type = "tag"
    tag_source = source
    source_conclusion = "success"
    def __init__(self, host, **_):
        self.host, self.sock = host, None
        self.response = None
    def request(self, method, path, body=None, headers=None):
        if self.host != "api.github.com":
            assert "Authorization" not in headers
            self.response = Response(200, {})
            self.response.body = archives[int(path[1:])]
            return
        assert headers["Authorization"] == "Bearer fixture-token"
        if self.redirect:
            self.response = Response(302, {})
        elif "/git/ref/tags/" in path:
            self.response = Response(200, {"object": {"type": self.tag_type, "sha": "b" * 40}})
        elif "/git/tags/" in path:
            self.response = Response(200, {"tag": metadata["tag"], "object": {"type": "commit", "sha": self.tag_source}})
        elif path.endswith("/git/ref/heads/main"):
            self.response = Response(200, {"object": {"type": "commit", "sha": self.main_sha}})
        elif "/compare/" in path:
            self.response = Response(200, {"status": "ahead" if self.ancestor else "diverged", "merge_base_commit": {"sha": source if self.ancestor else "c" * 40}})
        elif "/actions/runs/30/attempts/1/jobs?" in path:
            self.response = Response(200, {"total_count": 1, "jobs": [{"id": 301, "run_id": 30, "head_sha": source, "name": "Source preview publication / Admit source preview", "status": "completed", "conclusion": self.source_conclusion}]})
        elif path.endswith("/actions/runs/30/attempts/1"):
            self.response = Response(200, {"id": 30, "run_attempt": 1, "head_sha": source, "head_branch": metadata["tag"], "event": "push", "path": publisher.WORKFLOW, "head_repository": {"full_name": publisher.REPOSITORY}})
        elif "/actions/runs/30/artifacts?" in path:
            self.response = Response(200, {"total_count": 0, "artifacts": []})
        elif "/actions/artifacts/" in path:
            identity = int(path.split("/actions/artifacts/")[1].split("/")[0])
            if path.endswith("/zip"):
                self.response = Response(302, {})
                self.response.location = "https://productionresultssa32.blob.core.windows.net/" + str(identity)
            else:
                archive = archives[identity]
                self.response = Response(200, {"id": identity, "name": "source-preview-source-1" if identity == 100 else "source-preview-qualified-payload-1", "size_in_bytes": len(archive), "digest": "sha256:" + publisher.digest(archive), "expired": False, "workflow_run": {"id": 30, "head_sha": source}})
        elif "/releases?" in path:
            self.response = Response(200, [self.release] if self.release else [])
        elif method == "POST" and path.endswith("/releases"):
            type(self).writes += 1
            type(self).release = dict(json.loads(body), id=77, assets=[])
            self.response = Response(201, self.release)
        elif path.endswith("/releases/77"):
            self.response = Response(200, self.release)
        else:
            raise AssertionError((method, path))
    def getresponse(self):
        return self.response
    def close(self):
        pass

publisher.http.client.HTTPSConnection = ExternalNetwork

def case(name, change=None, reject=False, writes=1):
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        previous = Path.cwd()
        try:
            os.chdir(root)
            os.environ["RUNNER_TEMP"] = temporary
            (root / "source-preview-metadata").mkdir()
            (root / "source-preview-metadata/source-preview-metadata.json").write_bytes(publisher.canonical(metadata))
            (root / "source-preview-payload").mkdir()
            (root / "source-preview-payload" / publisher.PAYLOAD).write_bytes(PAYLOAD_BYTES)
            os.environ["GITHUB_OUTPUT"] = str(root / "output")
            ExternalNetwork.release = None
            ExternalNetwork.writes = 0
            ExternalNetwork.redirect = False
            ExternalNetwork.main_sha = source
            ExternalNetwork.ancestor = True
            ExternalNetwork.tag_type = "tag"
            ExternalNetwork.tag_source = source
            ExternalNetwork.source_conclusion = "success"
            if change:
                change(root)
            original = copy.deepcopy(ExternalNetwork.release)
            try:
                publisher.main()
            except (ValueError, OSError, KeyError, TypeError):
                assert reject
                assert ExternalNetwork.writes == 0
                assert ExternalNetwork.release == original
                return
            assert not reject, name + " was accepted"
            result = (root / "source-preview-decision.json").read_bytes()
            decision = json.loads(result)
            assert publisher.canonical(decision) == result
            assert decision["source"]["source_sha"] == source and decision["release_id"] == 77
            assert decision["candidate"] == candidate
            assert (root / "output").read_text() == "source_artifact_id=100\npayload_artifact_id=101\nsign=true\n"
            assert decision["payload"] == {"target": "x86_64-unknown-linux-gnu", "name": publisher.PAYLOAD, "size": len(PAYLOAD_BYTES), "sha256": publisher.digest(PAYLOAD_BYTES)}
            assert ExternalNetwork.release["draft"] and ExternalNetwork.release["prerelease"]
            assert ExternalNetwork.writes == writes
        finally:
            os.chdir(previous)

case("valid draft")
for key, bad in (("AUTHORITY_REPOSITORY", "other/repository"), ("AUTHORITY_PATH", ".github/workflows/other.yml"),
                 ("AUTHORITY_REF", "tailrocks/velnor-new/.github/workflows/source-preview-publication.yml@main"),
                 ("AUTHORITY_SHA", "D" * 40)):
    original = os.environ[key]
    os.environ[key] = bad
    case("invalid publication authority", reject=True)
    os.environ[key] = original
for key, bad in (("AUTHORITY_REPOSITORY", "other/repository"), ("AUTHORITY_PATH", ".github/workflows/other.yml"), ("AUTHORITY_SHA", "D" * 40)):
    original = os.environ[key]
    original_ref = os.environ["AUTHORITY_REF"]
    os.environ[key] = bad
    os.environ["AUTHORITY_REF"] = os.environ["AUTHORITY_REPOSITORY"] + "/" + os.environ["AUTHORITY_PATH"] + "@" + os.environ["AUTHORITY_SHA"]
    try:
        case("coherent invalid publication authority " + key, reject=True)
    finally:
        os.environ[key] = original
        os.environ["AUTHORITY_REF"] = original_ref
original = os.environ.pop("AUTHORITY_SHA")
case("missing publication authority", reject=True)
os.environ["AUTHORITY_SHA"] = original
case("advanced main with source ancestor", lambda _: setattr(ExternalNetwork, "main_sha", "b" * 40))
case("source outside protected main history", lambda _: setattr(ExternalNetwork, "ancestor", False), True)
case("lightweight tag", lambda _: setattr(ExternalNetwork, "tag_type", "commit"), True)
case("wrong tag commit", lambda _: setattr(ExternalNetwork, "tag_source", "b" * 40), True)
case("redirected authority", lambda _: setattr(ExternalNetwork, "redirect", True), True)
case("failed original source job", lambda _: setattr(ExternalNetwork, "source_conclusion", "failure"), True)
def foreign_draft(_):
    ExternalNetwork.release = {"id": 77, "tag_name": metadata["tag"], "target_commitish": source, "body": "foreign", "draft": True, "prerelease": True, "assets": []}
case("foreign draft", foreign_draft, True)
def matching_draft(_):
    ExternalNetwork.release = {"id": 77, "tag_name": metadata["tag"], "target_commitish": source, "body": publisher.candidate_marker(metadata, candidate), "draft": True, "prerelease": True, "assets": []}
case("matching draft retry", matching_draft, writes=0)
os.environ["GITHUB_RUN_ATTEMPT"] = "2"
os.environ["SOURCE_ARTIFACT_ID"], os.environ["PAYLOAD_ARTIFACT_ID"] = "103", "104"
case("full rerun retains original candidate", matching_draft, writes=0)
os.environ["GITHUB_RUN_ATTEMPT"] = "1"

os.environ["SOURCE_ARTIFACT_ID"], os.environ["PAYLOAD_ARTIFACT_ID"] = "100", "101"
def foreign_asset(root):
    matching_draft(root)
    ExternalNetwork.release["assets"] = [{"name": "foreign", "state": "uploaded"}]
case("foreign asset", foreign_asset, True)
def symlink(root):
    file = root / "source-preview-payload" / publisher.PAYLOAD
    file.unlink()
    file.symlink_to(root / "source-preview-metadata/source-preview-metadata.json")
case("symlink payload", symlink, True)


class ProtocolNetwork(ExternalNetwork):
    mutate = None

    def __init__(self, host, **kwargs):
        self.host = host
        self.response = None

    def request(self, method, path, body=None, headers=None):
        if self.host == "uploads.github.com":
            assert method == "POST" and headers["Authorization"] == "Bearer fixture-token"
            name = path.split("?name=", 1)[1]
            asset = {"name": name, "state": "uploaded", "size": len(body), "digest": "sha256:" + publisher.digest(body)}
            type(self).release["assets"].append(asset)
            type(self).writes += 1
            self.response = Response(201, asset)
        elif method == "PATCH":
            assert path.endswith("/releases/77") and json.loads(body) == {"draft": False}
            type(self).writes += 1
            type(self).release["draft"] = False
            type(self).release["immutable"] = True
            if self.mutate == "missing":
                type(self).release["assets"].pop()
            elif self.mutate == "changed":
                type(self).release["assets"][0]["digest"] = "sha256:" + "f" * 64
            self.response = Response(200, type(self).release)
        else:
            super().request(method, path, body, headers)


publisher.http.client.HTTPSConnection = ProtocolNetwork
payload = PAYLOAD_BYTES
receipt = dict(metadata, candidate=candidate, draft=metadata["source"], signer=metadata["source"], release_id=77, payload={"target": "x86_64-unknown-linux-gnu", "name": publisher.PAYLOAD, "size": len(payload), "sha256": publisher.digest(payload)})
receipt_bytes = publisher.canonical(receipt)
bundle = b"prevalidated bundle fixture"


def protocol_case(mutate=None, published=False, partial=False, reject=False, corrupt_partial=False):
    ProtocolNetwork.release = {"id": 77, "tag_name": metadata["tag"], "body": publisher.candidate_marker(metadata, candidate), "draft": not published, "prerelease": True, "immutable": published, "assets": []}
    ProtocolNetwork.writes = 0
    ProtocolNetwork.mutate = mutate
    expected = {publisher.RECEIPT: receipt_bytes, publisher.BUNDLE: bundle, publisher.PAYLOAD: payload}
    if partial or published:
        for name, data in list(expected.items())[:1 if partial else 3]:
            ProtocolNetwork.release["assets"].append({"name": name, "state": "uploaded", "size": len(data), "digest": "sha256:" + publisher.digest(data)})
    if published and mutate:
        ProtocolNetwork.release["assets"].pop()
    if corrupt_partial:
        ProtocolNetwork.release["assets"][0]["digest"] = "sha256:" + "f" * 64
    try:
        publisher.publish_verified(publisher.GitHub(), metadata, receipt, receipt_bytes, bundle, payload)
    except ValueError:
        assert reject
    else:
        assert not reject
        assert ProtocolNetwork.release["immutable"] is True
        assert publisher.existing_assets(ProtocolNetwork.release, expected) == set(expected)
    if published or corrupt_partial:
        assert ProtocolNetwork.writes == 0


protocol_case()
protocol_case(partial=True)
protocol_case(partial=True, corrupt_partial=True, reject=True)
protocol_case(mutate="missing", reject=True)
protocol_case(mutate="changed", reject=True)
protocol_case(published=True)
protocol_case(published=True, mutate="missing", reject=True)

source2 = dict(metadata)
candidate = {"source_artifact_id": 100, "payload_artifact_id": 101}
decision = {"source": source2, "candidate": candidate, "release_id": 77, "payload": receipt["payload"], "draft": dict(source2["source"])}
final = json.loads(publisher.finalize_receipt(decision, 2))
assert final["source"]["run_attempt"] == final["draft"]["run_attempt"] == 1
assert final["signer"]["run_attempt"] == 2
assert json.loads(publisher.candidate_marker(source2, candidate).split(":", 1)[1])["candidate"] == candidate
pair1 = {"receipt": b"receipt-1", "bundle": b"bundle-1", "signer_attempt": 1}
pair2 = {"receipt": b"receipt-2", "bundle": b"bundle-2", "signer_attempt": 2}
empty = {"draft": True, "assets": []}
assert publisher.select_pair(empty, [], payload) is None
assert publisher.select_pair(empty, [pair2, pair1], payload) == pair1
asset = {"name": publisher.RECEIPT, "state": "uploaded", "size": len(pair1["receipt"]), "digest": "sha256:" + publisher.digest(pair1["receipt"])}
assert publisher.select_pair({"draft": True, "assets": [asset]}, [pair2, pair1], payload) == pair1
for item, pairs in [({"draft": True, "assets": [asset]}, [pair2]), ({"draft": True, "assets": [asset]}, []), (empty, [pair1, pair1]), ({"draft": False, "immutable": True, "assets": [asset]}, [pair1])]:
    try:
        publisher.select_pair(item, pairs, payload)
    except ValueError:
        pass
    else:
        raise AssertionError("invalid signed-pair recovery accepted")


class RoleHTTP:
    conclusion = "success"
    def __init__(self, host, **kwargs):
        assert host == "api.github.com"
    def request(self, method, path, body=None, headers=None):
        assert method == "GET" and headers["Authorization"] == "Bearer fixture-token"
        if "/jobs?" in path:
            self.response = Response(200, {"total_count": 1, "jobs": [{"id": 41, "run_id": 30, "head_sha": source, "name": "Source preview publication / Attest source preview", "status": "completed", "conclusion": self.conclusion}]})
        else:
            self.response = Response(200, {"id": 30, "run_attempt": 1, "head_sha": source, "head_branch": metadata["tag"], "event": "push", "path": publisher.WORKFLOW, "head_repository": {"full_name": publisher.REPOSITORY}, "status": "completed", "conclusion": "failure"})
    def getresponse(self):
        return self.response
    def close(self):
        pass


publisher.http.client.HTTPSConnection = RoleHTTP
assert publisher.successful_job(publisher.GitHub(), source, 30, 1, "Source preview publication / Attest source preview") == 41
RoleHTTP.conclusion = "failure"
try:
    publisher.successful_job(publisher.GitHub(), source, 30, 1, "Source preview publication / Attest source preview")
except ValueError:
    pass
else:
    raise AssertionError("failed attestation role accepted")


class PrepareHTTP(RoleHTTP):
    def request(self, method, path, body=None, headers=None):
        assert method == "GET" and headers["Authorization"] == "Bearer fixture-token"
        if path.endswith("/releases/77"):
            self.response = Response(200, {"id": 77, "tag_name": metadata["tag"], "body": publisher.candidate_marker(metadata, candidate), "prerelease": True, "draft": True, "assets": []})
        elif "/artifacts?" in path:
            self.response = Response(200, {"total_count": 1, "artifacts": [{"id": 902, "name": "source-preview-signed-1", "expired": False, "workflow_run": {"id": 30, "head_sha": source}}]})
        elif "/jobs?" in path:
            self.response = Response(200, {"total_count": 3, "jobs": [{"id": 41 + index, "run_id": 30, "head_sha": source, "name": name, "status": "completed", "conclusion": conclusion} for index, (name, conclusion) in enumerate([("Source preview publication / Admit source preview", "success"), ("Source preview publication / Draft source preview", "success"), ("Source preview publication / Attest source preview", "failure")])]})
        else:
            super().request(method, path, body, headers)


publisher.http.client.HTTPSConnection = PrepareHTTP
os.environ["GITHUB_RUN_ATTEMPT"] = "2"
prepared = publisher.prepare_pair(publisher.GitHub(), source2, decision, payload)
assert prepared["sign"] is True
assert json.loads(publisher.finalize_receipt(decision, 2))["signer"]["run_attempt"] == 2
os.environ["GITHUB_RUN_ATTEMPT"] = "1"


class AttestHTTP(ExternalNetwork):
    def request(self, method, path, body=None, headers=None):
        assert "/releases/" not in path
        if "/jobs?" in path:
            self.response = Response(200, {"total_count": 2, "jobs": [{"id": 41 + index, "run_id": 30, "head_sha": source, "name": name, "status": "completed", "conclusion": "success"} for index, name in enumerate(("Source preview publication / Admit source preview", "Source preview publication / Draft source preview"))]})
        else:
            super().request(method, path, body, headers)


with tempfile.TemporaryDirectory() as temporary:
    previous = Path.cwd()
    try:
        os.chdir(temporary)
        root = Path(temporary)
        for directory in ("source-preview-metadata", "source-preview-payload", "source-preview-decision"):
            (root / directory).mkdir()
        (root / "source-preview-metadata/source-preview-metadata.json").write_bytes(publisher.canonical(source2))
        (root / "source-preview-payload" / publisher.PAYLOAD).write_bytes(payload)
        (root / "source-preview-decision/source-preview-decision.json").write_bytes(publisher.canonical(decision))
        os.environ.update({"RUNNER_TEMP": temporary, "GITHUB_OUTPUT": str(root / "output"), "GITHUB_RUN_ATTEMPT": "2", "PREVIEW_MODE": "prepare", "SIGN": "true"})
        publisher.http.client.HTTPSConnection = AttestHTTP
        publisher.main()
        assert json.loads((root / publisher.RECEIPT).read_bytes())["signer"]["run_attempt"] == 2
        assert (root / "output").read_text() == "sign=true\n"
    finally:
        os.chdir(previous)
