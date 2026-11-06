# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import copy
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile

spec = importlib.util.spec_from_file_location("preview_publication", sys.argv[1])
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)
source = "a" * 40
signer_path = publisher.WORKFLOW

def run(path, run_id):
    return {"workflow_path": path, "workflow_ref": publisher.REPOSITORY + "/" + path + "@refs/heads/main", "workflow_sha": source, "run_id": run_id, "run_attempt": 1}

tag_ref = "refs/tags/sdk-preview-" + source
metadata = {"schema": 1, "repository": publisher.REPOSITORY, "version": "0.1.75", "source_sha": source, "tag": "sdk-preview-" + source, "signer": run(signer_path, 30), "ci": run(".github/workflows/ci.yml", 10), "qualification": run(".github/workflows/source-qualification.yml", 20)}
metadata["signer"]["workflow_ref"] = publisher.REPOSITORY + "/" + publisher.WORKFLOW + "@" + tag_ref
os.environ.update({"GITHUB_SHA": source, "GITHUB_REPOSITORY": publisher.REPOSITORY, "GITHUB_EVENT_NAME": "push", "GITHUB_REF": tag_ref, "GITHUB_WORKFLOW_REF": metadata["signer"]["workflow_ref"], "GITHUB_WORKFLOW_SHA": source, "GITHUB_RUN_ID": "30", "GITHUB_RUN_ATTEMPT": "1", "PREVIEW_MODE": "draft", "GH_TOKEN": "fixture-token"})

class Response:
    def __init__(self, status, body):
        self.status, self.body = status, publisher.canonical(body)
    def read(self, limit):
        result, self.body = self.body[:limit], self.body[limit:]
        return result

class ExternalNetwork:
    release = None
    writes = 0
    redirect = False
    main_sha = source
    ancestor = True
    tag_type = "tag"
    tag_source = source
    def __init__(self, host, **_):
        assert host == "api.github.com"
        self.response = None
    def request(self, method, path, body=None, headers=None):
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
        elif "/releases/tags/" in path:
            self.response = Response(200 if self.release else 404, self.release or {})
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
            (root / "source-preview-payload" / publisher.PAYLOAD).write_bytes(b"qualified-payload")
            ExternalNetwork.release = None
            ExternalNetwork.writes = 0
            ExternalNetwork.redirect = False
            ExternalNetwork.main_sha = source
            ExternalNetwork.ancestor = True
            ExternalNetwork.tag_type = "tag"
            ExternalNetwork.tag_source = source
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
            result = (root / publisher.RECEIPT).read_bytes()
            receipt = json.loads(result)
            assert publisher.canonical(receipt) == result
            assert receipt["source_sha"] == source and receipt["release_id"] == 77
            assert receipt["payload"] == {"target": "x86_64-unknown-linux-gnu", "name": publisher.PAYLOAD, "size": len(b"qualified-payload"), "sha256": publisher.digest(b"qualified-payload")}
            assert ExternalNetwork.release["draft"] and ExternalNetwork.release["prerelease"]
            assert ExternalNetwork.writes == writes
        finally:
            os.chdir(previous)

case("valid draft")
case("advanced main with source ancestor", lambda _: setattr(ExternalNetwork, "main_sha", "b" * 40))
case("source outside protected main history", lambda _: setattr(ExternalNetwork, "ancestor", False), True)
case("lightweight tag", lambda _: setattr(ExternalNetwork, "tag_type", "commit"), True)
case("wrong tag commit", lambda _: setattr(ExternalNetwork, "tag_source", "b" * 40), True)
case("redirected authority", lambda _: setattr(ExternalNetwork, "redirect", True), True)
def foreign_draft(_):
    ExternalNetwork.release = {"id": 77, "tag_name": metadata["tag"], "target_commitish": source, "body": "foreign", "draft": True, "prerelease": True, "assets": []}
case("foreign draft", foreign_draft, True)
def matching_draft(_):
    ExternalNetwork.release = {"id": 77, "tag_name": metadata["tag"], "target_commitish": source, "body": "velnor-source-preview:30:1:" + source, "draft": True, "prerelease": True, "assets": []}
case("matching draft retry", matching_draft, writes=0)
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
payload = b"qualified-payload"
receipt = dict(metadata, release_id=77, payload={"target": "x86_64-unknown-linux-gnu", "name": publisher.PAYLOAD, "size": len(payload), "sha256": publisher.digest(payload)})
receipt_bytes = publisher.canonical(receipt)
bundle = b"prevalidated bundle fixture"


def protocol_case(mutate=None, published=False, partial=False, reject=False, corrupt_partial=False):
    ProtocolNetwork.release = {"id": 77, "tag_name": metadata["tag"], "body": "velnor-source-preview:30:1:" + source, "draft": not published, "prerelease": True, "immutable": published, "assets": []}
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
