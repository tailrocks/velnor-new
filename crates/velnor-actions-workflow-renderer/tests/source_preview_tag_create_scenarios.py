# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
from datetime import datetime, timedelta, timezone
import hashlib
import io
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import stat
import zipfile

program = Path(sys.argv[1]).resolve()
spec = importlib.util.spec_from_file_location("sdk_tag_create", program)
p = importlib.util.module_from_spec(spec)
spec.loader.exec_module(p)
source = "a" * 40
trusted_revision = "d" * 40
tag = "sdk-preview-" + source
repo = p.REPOSITORY
prefix = "/repos/" + repo
workflow = ".github/workflows/product-release-generator.yml"
ci = ".github/workflows/ci.yml"
qualification = ".github/workflows/source-qualification.yml"
names = {10: ["Required verdict"], 20: ["Source qualification ordinary harness", "Source qualification keep harness", "Source qualification generation and repository checks", "Source qualification"], 30: ["Build source preview Linux", "Build source preview Darwin", "Qualify source preview Linux", "Qualify source preview Darwin"]}
payload_name = "velnor-actions-x86_64-unknown-linux-gnu"

def archive(invalid=False, darwin=False):
    name = "velnor-actions-aarch64-apple-darwin" if darwin else payload_name
    valid = bytes.fromhex("cffaedfe0c0000010000000002000000") + bytes(16) + b"fixture" if darwin else b"\x7fELF\x02\x01\x01" + b"\0" * 11 + b"\x3e\x00" + b"fixture"
    payload = b"x" * 30 if invalid else valid
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as output:
        item = zipfile.ZipInfo(name)
        item.create_system = 3
        item.external_attr = (stat.S_IFREG | 0o644) << 16
        output.writestr(item, payload)
    return buffer.getvalue()

def pinned_gh():
    for version in ("2.103.3", "2.103.2"):
        result = subprocess.run(["mise", "which", "gh", "--tool=gh@" + version], capture_output=True, text=True, check=False)
        if result.returncode == 0:
            path = Path(result.stdout.strip())
            if hashlib.sha256(path.read_bytes()).hexdigest() in p.GH_SHA256.get((p.platform.system(), p.platform.machine()), set()):
                return str(path)
    raise AssertionError("pinned gh is required")

def run(identity, path, event):
    return {"id": identity, "run_attempt": 1, "head_sha": source, "head_branch": "main", "event": event, "status": "completed", "conclusion": "success", "path": path, "head_repository": {"full_name": repo, "id": 1390620900}}

class Response:
    def __init__(self, status, body, location=""):
        self.status, self.body, self.location = status, body if isinstance(body, bytes) else json.dumps(body).encode(), location
    def read(self, limit):
        data, self.body = self.body[:limit], self.body[limit:]
        return data
    def getheader(self, name, default=None):
        return self.location if name == "Location" else default

class Network:
    variant = ""
    object = None
    reference = None
    immutable_reads = 0
    reference_reads = 0
    rules_reads = 0
    writes = []
    archive_data = b""
    darwin_archive_data = b""
    downloaded = False
    candidate_code_reads = 0
    def __init__(self, host, **_):
        self.host = host
        self.sock = None
    def request(self, method, path, body=None, headers=None):
        if self.host.endswith(".blob.core.windows.net"):
            assert method == "GET" and "Authorization" not in headers and "Cookie" not in headers
            type(self).downloaded = True
            self.response = Response(200, self.darwin_archive_data if path.startswith("/darwin") else self.archive_data)
            return
        assert self.host == "api.github.com"
        assert headers["Authorization"] == "Bearer fixture-token"
        status, value = 200, {}
        if method == "POST":
            type(self).writes.append(path)
            request = json.loads(body)
            if path == prefix + "/git/tags":
                type(self).object = request
                status, value = 201, {"sha": "b" * 40, "tag": tag, "object": {"type": "commit", "sha": source}}
            elif path == prefix + "/git/refs":
                assert request == {"ref": "refs/tags/" + tag, "sha": "b" * 40}
                if self.variant == "post-ref-error":
                    status = 422
                else:
                    type(self).reference = request
                    status, value = 201, {"ref": request["ref"], "object": {"type": "tag", "sha": request["sha"]}}
            else:
                raise AssertionError(path)
        elif path == "/user":
            value = {"id": 1 if self.variant == "owner" else 139017, "login": "donbeave"}
        elif path == prefix:
            value = {"id": 1 if self.variant == "repository-id" else 1390620900, "full_name": repo, "default_branch": "main", "owner": {"id": 1 if self.variant == "repository-owner" else 78806509}}
        elif path == prefix + "/git/ref/heads/main":
            value = {"object": {"type": "commit", "sha": "c" * 40 if self.variant == "main" else source}}
        elif path == prefix + "/rulesets/26696396":
            type(self).rules_reads += 1
            bad = self.variant == "rules" or (self.variant == "rules-recheck" and self.rules_reads > 1)
            value = {"id": 26696396, "target": "tag", "enforcement": "active", "bypass_actors": [] if bad else [{"actor_id": 139017, "actor_type": "User", "bypass_mode": "always"}], "conditions": {"ref_name": {"include": ["refs/tags/sdk-preview-*"], "exclude": []}}, "rules": [{"type": "creation"}]}
        elif path == prefix + "/rulesets/24397132":
            value = {"id": 24397132, "target": "tag", "enforcement": "active", "bypass_actors": [], "conditions": {"ref_name": {"include": ["~ALL"], "exclude": []}}, "rules": [{"type": "update"}, {"type": "deletion"}]}
        elif path == prefix + "/compare/" + source + "..." + source:
            value = {"status": "identical", "merge_base_commit": {"sha": source}}
        elif path == prefix + "/compare/" + trusted_revision + "..." + source:
            value = {"status": "diverged" if self.variant == "untrusted-revision" else "ahead", "merge_base_commit": {"sha": trusted_revision}}
        elif "/contents/crates/velnor-actions-workflow-renderer/src/" in path:
            name = path.split("/src/", 1)[1].split("?", 1)[0]
            if path.endswith("?ref=" + source):
                type(self).candidate_code_reads += 1
                code = b"raise RuntimeError('candidate Python executed in tag authority')\n"
            else:
                assert path.endswith("?ref=" + trusted_revision)
                code = (program.parent / name).read_bytes()
            value = {"path": "crates/velnor-actions-workflow-renderer/src/" + name, "encoding": "base64", "content": base64.b64encode(code).decode()}
        elif path == prefix + "/contents/Cargo.toml?ref=" + source:
            value = {"path": "Cargo.toml", "encoding": "base64", "content": base64.b64encode(b'[workspace.package]\nversion="0.1.75"\n').decode()}
        elif "/actions/workflows/" in path:
            if "source-qualification.yml" in path:
                selected = run(20, qualification, "workflow_dispatch")
            elif "product-release-generator.yml" in path:
                selected = run(30, workflow, "push")
            else:
                selected = run(10, ci, "push")
            if self.variant == "producer" and selected["id"] == 30:
                selected["conclusion"] = "failure"
            value = {"workflow_runs": [selected]}
        elif "/actions/runs/" in path and "/jobs?" in path:
            identity = int(path.split("/actions/runs/")[1].split("/")[0])
            jobs = [{"id": identity * 100 + i, "name": name, "status": "completed", "conclusion": "success", "run_id": identity, "run_attempt": 1, "head_sha": source} for i, name in enumerate(names[identity])]
            value = {"total_count": len(jobs), "jobs": jobs}
        elif path == prefix + "/actions/runs/30/attempts/1":
            value = run(30, workflow, "push")
        elif path == prefix + "/actions/runs/30":
            value = run(30, workflow, "push")
            if self.variant == "producer-race" and self.downloaded:
                value["run_attempt"] = 2
        elif path == prefix + "/actions/runs/30/artifacts?per_page=100":
            digest = hashlib.sha256(self.archive_data).hexdigest()
            if self.variant == "artifact": digest = "z" * 64
            if self.variant == "archive-digest": digest = "0" * 64
            item = {"id": 40, "name": "source-preview-payload-linux-1", "expired": False, "size_in_bytes": len(self.archive_data), "digest": "sha256:" + digest, "workflow_run": {"id": 30, "head_sha": source}}
            other = {"id": 41, "name": "source-preview-payload-darwin-1", "expired": False, "size_in_bytes": len(self.darwin_archive_data), "digest": "sha256:" + hashlib.sha256(self.darwin_archive_data).hexdigest(), "workflow_run": {"id": 30, "head_sha": source}}
            value = {"total_count": 2, "artifacts": [item, other]}
        elif path in (prefix + "/actions/artifacts/40/zip", prefix + "/actions/artifacts/41/zip"):
            name = "darwin" if "/41/" in path else "linux"
            self.response = Response(302, {}, "https://productionresultssa32.blob.core.windows.net/" + name + "?sig=secret")
            return
        elif path == prefix + "/git/ref/tags/" + tag:
            type(self).reference_reads += 1
            if self.variant == "existing" or (self.variant == "ref-race" and self.reference_reads > 1):
                value = {"ref": "refs/tags/" + tag}
            else:
                status = 404
        elif path == prefix + "/immutable-releases":
            type(self).immutable_reads += 1
            value = {"enabled": self.variant != "immutable" and not (self.variant == "immutable-recheck" and self.immutable_reads > 1)}
        else:
            raise AssertionError((method, path))
        self.response = Response(status, value)
    def getresponse(self):
        return self.response
    def close(self):
        pass

p.http.client.HTTPSConnection = Network
with tempfile.TemporaryDirectory() as temporary:
    (Path(temporary) / "hosts.yml").write_text("github.com:\n  oauth_token: fixture-token\n  user: fixture\n")
    os.environ["GH_CONFIG_DIR"] = temporary
    gh = pinned_gh()
    for variant in ("valid", "owner", "repository-id", "repository-owner", "main", "untrusted-revision", "self-authority", "invalid-authority", "rules", "rules-recheck", "producer", "payload-invalid", "producer-race", "artifact", "archive-digest", "existing", "immutable", "immutable-recheck", "ref-race", "post-ref-error", "stale"):
        Network.variant, Network.object, Network.reference = variant, None, None
        Network.downloaded = False
        Network.candidate_code_reads = 0
        Network.archive_data = archive(invalid=variant == "payload-invalid")
        Network.darwin_archive_data = archive(darwin=True)
        Network.immutable_reads, Network.reference_reads, Network.rules_reads, Network.writes = 0, 0, 0, []
        clock_reads = [0]
        def fixture_now():
            clock_reads[0] += 1
            offset = 61 if variant == "stale" and clock_reads[0] >= 3 else 0
            return datetime(2026, 11, 6, 0, 0, 0, tzinfo=timezone.utc) + timedelta(seconds=offset)
        p.now_utc = fixture_now
        try:
            selected_authority = source if variant == "self-authority" else "main" if variant == "invalid-authority" else trusted_revision
            p.create(source, gh, selected_authority)
        except (ValueError, OSError, KeyError, TypeError):
            assert variant not in ("valid", "producer-race"), "authorized tag was rejected"
        else:
            assert variant in ("valid", "producer-race"), variant + " was accepted"
        if variant in ("valid", "producer-race"):
            assert Network.reference == {"ref": "refs/tags/" + tag, "sha": "b" * 40}
            message = Network.object["message"]
            record = json.loads(message)
            assert p.canonical(record).decode() == message
            assert record["targets"]["linux"]["producer_run_id"] == 30 and record["targets"]["linux"]["payload_artifact_id"] == 40 and record["targets"]["linux"]["payload_archive_sha256"] == hashlib.sha256(Network.archive_data).hexdigest()
            assert record["targets"]["darwin"]["payload_artifact_id"] == 41 and record["targets"]["darwin"]["payload_archive_sha256"] == hashlib.sha256(Network.darwin_archive_data).hexdigest()
            assert record["immutability"] == {"enabled": True, "checked_at": "2026-11-06T00:00:00Z"}
        else:
            assert Network.reference is None, variant + " created a tag ref"
        assert Network.candidate_code_reads == 0, "candidate Python reached the token-bearing owner process"
