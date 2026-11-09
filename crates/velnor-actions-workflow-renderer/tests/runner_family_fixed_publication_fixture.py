# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import base64
import hashlib
import contextlib
import io
import os
import runpy
import json
import sys
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

root = Path(sys.argv[1])
sys.path.insert(0, str(root))
import job_image_authority_transport as transport
import job_image_authority_registry as registry_module
import job_image_authority_publish as publisher
import runner_image_authority_candidate as candidate
import runner_image_authority_custody as custody
import runner_image_authority_identity as identity
import runner_image_authority_publish as publication

case = json.loads((root / "case.json").read_text())
scenario = case.get("scenario", "publish")
parts = {name: bytes.fromhex(value) for name, value in case["parts"].items()}
archive = root / "candidate.oci.tar"
lock_raw = (root / "runner.lock").read_bytes()
lock_sha = hashlib.sha256(lock_raw).hexdigest()
source = "a" * 40
inputs = {".velnor/runner.lock": lock_sha, "images/runner/ubuntu-26.04/Dockerfile": "c" * 64}
graph = {
    "archive_index_digest": hashlib.sha256(parts["index"]).hexdigest(),
    "archive_index_size_bytes": len(parts["index"]),
    "platform_manifest_digest": "sha256:" + hashlib.sha256(parts["manifest"]).hexdigest(),
    "platform_manifest_size_bytes": len(parts["manifest"]),
    "image_configuration_digest": "sha256:" + hashlib.sha256(parts["config"]).hexdigest(),
    "image_configuration_size_bytes": len(parts["config"]),
    "layer_digests": ["sha256:" + hashlib.sha256(parts["layer"]).hexdigest()],
}
archive_info = {"sha256": hashlib.sha256(archive.read_bytes()).hexdigest(), "size_bytes": archive.stat().st_size}
role_archives = {role: root / f"{role}.oci.tar" for role in ("runner", "dind", "resource_probe")}
for role_archive in role_archives.values():
    role_archive.write_bytes(archive.read_bytes())
context = {"path": "target/velnor-runner-family/resource_probe", "sha256": "b" * 64, "size_bytes": 1}
versions = {"runner_version": "2.338.0", "docker_version": "29.9.1", "buildx_version": "0.38.0", "compose_version": "5.6.0", "node_version": "26.12.0"}
roles = {}
records = {}
envelopes = {}
for role, image in (("runner", "velnor-runner"), ("dind", "velnor-dind"), ("resource_probe", "velnor-resource-probe")):
    statement = {"_type": "https://in-toto.io/Statement/v1", "predicateType": "https://slsa.dev/provenance/v1", "subject": [{"name": f"pkg:docker/{image}@linux-amd64?platform=linux%2Famd64", "digest": {"sha256": graph["platform_manifest_digest"][7:]}}]}
    envelope = {"schema": "velnor.runner-provenance-envelope.v1", "original_archive_sha256": archive_info["sha256"], "original_archive_size_bytes": archive_info["size_bytes"], "original_index_b64": base64.b64encode(parts["index"]).decode(), "buildkit_index_b64": base64.b64encode(parts["index"]).decode(), "attestation_descriptor": {}, "attestation_manifest_b64": "e30=", "attestation_config_b64": "e30=", "statement_b64": base64.b64encode(json.dumps(statement, separators=(",", ":")).encode()).decode()}
    envelope_raw = json.dumps(envelope, separators=(",", ":")).encode()
    provenance = {"envelope": {"sha256": hashlib.sha256(envelope_raw).hexdigest(), "size_bytes": len(envelope_raw)}, "original_archive": archive_info, "original_index": {"sha256": hashlib.sha256(parts["index"]).hexdigest(), "size_bytes": len(parts["index"])}, "named_contexts": {"resource_probe": {**context, "sha256": "d" * 64 if scenario == "different_probe_context" and role == "resource_probe" else context["sha256"]}}}
    record = {"schema": "velnor.runner-image-publication.v2", "image": role, "source": {"commit": source, "ref": "refs/heads/main", "inputs": inputs}, "workflow": {"repository": "tailrocks/velnor-new", "repository_id": 1390620900, "event": "push", "workflow_ref": "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main", "authority_commit": source, "run_id": 101, "run_attempt": 1}, "platform": "linux/amd64", "archive": archive_info, "oci": graph, "provenance": provenance, "lock": {"sha256": lock_sha, **versions}}
    record_raw = json.dumps(record, separators=(",", ":")).encode()
    records[role] = record_raw.decode()
    envelopes[role] = envelope_raw.decode()
    roles[role] = {"record_sha256": hashlib.sha256(record_raw).hexdigest(), "provenance": provenance, "archive": archive_info, "oci": graph}
receipt = {"schema": "velnor.runner-image-qualification.v2", "source_commit": source, "source_ref": "refs/heads/main", "runner_lock_sha256": lock_sha, "producer_run_id": 101, "producer_run_attempt": 1, "qualifier_run_id": 202, "qualifier_run_attempt": 2, "qualifier_job_id": 303, "images": roles}
receipt_raw = json.dumps(receipt, separators=(",", ":")).encode()
signer = "https://github.com/tailrocks/velnor-new/.github/workflows/runner-image-qualification-attest.yml@refs/heads/main"
caller = "https://github.com/tailrocks/velnor-new/.github/workflows/runner-image-qualification.yml@refs/heads/main"
certificate = {"issuer": "https://token.actions.githubusercontent.com", "subjectAlternativeName": signer, "sourceRepositoryURI": "https://github.com/tailrocks/velnor-new", "sourceRepositoryOwnerURI": "https://github.com/tailrocks", "sourceRepositoryIdentifier": "1390620900", "sourceRepositoryDigest": source, "sourceRepositoryRef": "refs/heads/main", "buildSignerURI": signer, "buildSignerDigest": source, "buildConfigURI": caller, "buildConfigDigest": source, "buildTrigger": "push", "runInvocationURI": "https://github.com/tailrocks/velnor-new/actions/runs/202/attempts/2", "runnerEnvironment": "github-hosted"}
verification = [{"verificationResult": {"signature": {"certificate": certificate}, "statement": {"predicateType": "https://slsa.dev/provenance/v1", "subject": [{"name": "RUNNER_IMAGE_QUALIFICATION.json", "digest": {"sha256": hashlib.sha256(receipt_raw).hexdigest()}}], "predicate": {"buildDefinition": {"internalParameters": {"github": {"event_name": "push"}}}}}}}]
if scenario == "invalid_signer":
    verification[0]["verificationResult"]["signature"]["certificate"]["buildSignerDigest"] = "c" * 40

class Response:
    def __init__(self, status, headers, body=b""):
        self.status = status
        self.headers = headers
        self.body = body
    def getheaders(self):
        return list(self.headers.items())
    def read(self, size=-1):
        if size < 0:
            size = len(self.body)
        value, self.body = self.body[:size], self.body[size:]
        return value
    def close(self):
        pass

class Network:
    store = {}
    puts = 0
    anonymous_reads = []
    anonymous_token_requests = []
    def __init__(self, host, timeout=None, context=None):
        self.host = host
        self.headers = {}
        self.sent = bytearray()
    def request(self, method, target, headers=None):
        self.method, self.target = method, target
    def putrequest(self, method, target):
        self.method, self.target = method, target
    def putheader(self, name, value):
        self.headers[name.lower()] = value
    def endheaders(self):
        pass
    def send(self, data):
        self.sent.extend(data)
    def close(self):
        pass
    def getresponse(self):
        target = self.target
        if self.host == "api.github.com" and self.method == "GET" and target == "/repos/tailrocks/velnor-new/git/ref/heads/main":
            return Response(200, {}, json.dumps({"ref": "refs/heads/main", "object": {"type": "commit", "sha": source}}).encode())
        if self.method == "GET" and target == "/v2/":
            return Response(401, {"www-authenticate": 'Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:fixture:pull,push"'})
        if self.method == "GET" and target.startswith("/token"):
            if "authorization" not in self.headers:
                Network.anonymous_token_requests.append(target)
                return Response(200, {}, b'{"token":"anonymous-token-1234567890"}')
            return Response(200, {}, b'{"token":"publisher-token-1234567890"}')
        if self.method == "POST" and target.endswith("/blobs/uploads/"):
            return Response(202, {"location": target + "stub"})
        key = (self.host, target.split("?", 1)[0])
        if self.method == "HEAD":
            data = self.store.get(key)
            if data is None:
                return Response(404, {})
            return Response(200, {"docker-content-digest": "sha256:" + hashlib.sha256(data).hexdigest(), "content-length": str(len(data))})
        if self.method == "PUT":
            Network.puts += 1
            body = bytes(self.sent)
            if "/blobs/uploads/" in target:
                digest = parse_qs(urlsplit(target).query)["digest"][0]
                key = (self.host, "/v2/" + target.split("/v2/", 1)[1].split("/blobs/uploads/", 1)[0] + "/blobs/" + digest)
            self.store[key] = body
            return Response(201, {"docker-content-digest": "sha256:" + hashlib.sha256(body).hexdigest()})
        if self.method == "GET":
            if self.headers.get("authorization") == "Bearer anonymous-token-1234567890":
                Network.anonymous_reads.append(target)
                if scenario.startswith("private_") and f"/velnor-{scenario.removeprefix('private_').replace('_', '-')}/" in target:
                    return Response(403, {})
                if scenario == "anonymous_bad_blob" and "/blobs/" in target:
                    original = self.store[key]
                    corrupted = bytes([original[0] ^ 1]) + original[1:]
                    return Response(200, {"content-length": str(len(corrupted))}, corrupted)
            data = self.store.get(key)
            return Response(200, {"docker-content-digest": "sha256:" + hashlib.sha256(data).hexdigest(), "content-length": str(len(data))}, data)
        raise AssertionError((self.method, target))

transport.http.client.HTTPSConnection = Network
transport.http.client.HTTPException = OSError
transports = {role: transport.RegistryTransport(30, publication.registry.RegistryPolicy(profile)) for role, profile in publication.PROFILES.items()}
inputs_for_roles = {role: {"archive": role_archives[role], "record": records[role].encode(), "envelope": envelopes[role].encode()} for role in records}
if scenario == "tampered_third_record":
    inputs_for_roles["resource_probe"]["record"] += b"\n"
if scenario == "tampered_third_archive":
    path = inputs_for_roles["resource_probe"]["archive"]
    changed = bytearray(path.read_bytes())
    header = bytearray(changed[:512])
    header[136:148] = b"00000000001\0"
    header[148:156] = b"        "
    checksum = sum(header)
    header[148:156] = f"{checksum:06o}\0 ".encode()
    changed[:512] = header
    path.write_bytes(changed)
tag_path = f"/v2/{publication.PROFILES['runner'].value}/manifests/sha-{source}"
preseed = b"fixed-conflicting-source-tag"
if scenario == "conflicting_source_tag":
    Network.store[("ghcr.io", tag_path)] = preseed
def current_source():
    runner_published = any(host == "ghcr.io" and path.startswith("/v2/tailrocks/velnor-runner/manifests/sha256:") for host, path in Network.store)
    if scenario == "stale_before_publish" or (scenario == "stale_after_image" and runner_published):
        return "c" * 40
    return source

def run_driver():
    names = {
        "runner": ("velnor-runner-linux-amd64.tar", "runner-image-publication.json", "runner-image-provenance.json"),
        "dind": ("velnor-dind-linux-amd64.tar", "dind-image-publication.json", "dind-image-provenance.json"),
        "resource_probe": ("velnor-resource-probe-linux-amd64.tar", "resource-probe-image-publication.json", "resource-probe-image-provenance.json"),
    }
    candidate_dir = root / "runner-family-candidate"
    qualified = root / "runner-family-qualification"
    authenticated = root / "runner-family-authenticated"
    for directory in (candidate_dir, qualified, authenticated):
        directory.mkdir(mode=0o700)
    for role, (archive_name, record_name, envelope_name) in names.items():
        (candidate_dir / archive_name).write_bytes(role_archives[role].read_bytes())
        (candidate_dir / record_name).write_text(records[role])
        (candidate_dir / envelope_name).write_text(envelopes[role])
    (qualified / identity.RECEIPT_NAME).write_bytes(receipt_raw)
    (authenticated / "qualification-verification.json").write_text(json.dumps(verification))
    (authenticated / "runner.lock").write_bytes(lock_raw)
    (authenticated / "LICENSE").write_bytes(b"fixture license\n")
    (authenticated / "NOTICE").write_bytes(b"fixture notice\n")
    assets = root / "runner-family-release" / "assets"
    sentinel = b"existing manifest sentinel"
    if scenario == "driver_existing":
        assets.mkdir(parents=True)
        (assets / "RUNNER_IMAGE_MANIFEST.json").write_bytes(sentinel)
    os.environ.update({"RUNNER_TEMP": str(root), "GITHUB_SHA": source, "GITHUB_REPOSITORY": identity.REPOSITORY,
        "GITHUB_REPOSITORY_ID": identity.REPOSITORY_ID, "GITHUB_REF": "refs/heads/main", "GITHUB_EVENT_NAME": "push",
        "GITHUB_WORKFLOW_REF": identity.REPOSITORY + "/.github/workflows/product-release-images.yml@refs/heads/main",
        "GITHUB_WORKFLOW_SHA": source, "PRODUCER_RUN_ID": "101", "PRODUCER_RUN_ATTEMPT": "1",
        "QUALIFIER_RUN_ID": "202", "QUALIFIER_RUN_ATTEMPT": "2", "QUALIFIER_JOB_ID": "303",
        "GITHUB_ACTOR": "actor", "GH_TOKEN": "password"})
    stderr = io.StringIO()
    code = 0
    with contextlib.redirect_stderr(stderr):
        try:
            runpy.run_path(str(root / "runner_image_authority_driver.py"), run_name="__main__")
        except SystemExit as error:
            code = error.code
    result = {"driver_exit": code, "driver_stderr": stderr.getvalue()}
    if scenario == "driver_existing":
        result["sentinel_preserved"] = (assets / "RUNNER_IMAGE_MANIFEST.json").read_bytes() == sentinel
    elif code == 0:
        expected = {"RUNNER_IMAGE_MANIFEST.json", "SHA256SUMS", "LICENSE", "NOTICE"}
        files = {p.name: p.read_bytes() for p in assets.iterdir()}
        sums = "".join(f"{hashlib.sha256(files[name]).hexdigest()}  {name}\n" for name in ("RUNNER_IMAGE_MANIFEST.json", "LICENSE", "NOTICE")).encode()
        result["files_valid"] = (set(files) == expected and files["SHA256SUMS"] == sums
            and files["LICENSE"] == b"fixture license\n" and files["NOTICE"] == b"fixture notice\n")
        (root / "manifest.json").write_bytes(files["RUNNER_IMAGE_MANIFEST.json"])
    result["source_tags"] = {}
    for role, profile in publication.PROFILES.items():
        target = f"/v2/{profile.value}/manifests/sha-{source}"
        body = Network.store.get(("ghcr.io", target))
        result["source_tags"][role] = None if body is None else hashlib.sha256(body).hexdigest()
    (root / "result.json").write_text(json.dumps(result))


if scenario in ("driver_fresh", "driver_existing"):
    run_driver()
    raise SystemExit(0)

try:
    manifest = publication.publish_family(inputs_for_roles, receipt_raw, json.dumps(verification).encode(), source, 101, 1, 202, 2, 303, lock_raw, root / "private", "actor", "password", transports, current_source)
    (root / "manifest.json").write_bytes(manifest)
    result = {"ok": True}
except (publication.AuthorityError, publisher.AuthorityError, identity.AuthorityError, candidate.AuthorityError, custody.AuthorityError, registry_module.AuthorityError) as error:
    result = {"ok": False, "error": str(error)}
result["anonymous_token_requests"] = Network.anonymous_token_requests
result["anonymous_reads"] = Network.anonymous_reads
result["registry_puts"] = Network.puts
result["stored_paths"] = sorted(path for host, path in Network.store if host == "ghcr.io")
if scenario == "tampered_third_archive":
    result["archive_mutation"] = {
        "qualified_sha256": archive_info["sha256"],
        "qualified_size_bytes": archive_info["size_bytes"],
        "runner_sha256": hashlib.sha256(role_archives["runner"].read_bytes()).hexdigest(),
        "dind_sha256": hashlib.sha256(role_archives["dind"].read_bytes()).hexdigest(),
        "resource_probe_sha256": hashlib.sha256(role_archives["resource_probe"].read_bytes()).hexdigest(),
        "resource_probe_size_bytes": role_archives["resource_probe"].stat().st_size,
    }
result["source_tags"] = {}
for role, profile in publication.PROFILES.items():
    target = f"/v2/{profile.value}/manifests/sha-{source}"
    body = Network.store.get(("ghcr.io", target))
    result["source_tags"][role] = None if body is None else hashlib.sha256(body).hexdigest()
if scenario == "conflicting_source_tag":
    result["preseed_sha256"] = hashlib.sha256(preseed).hexdigest()
    result["runner_tag_after_sha256"] = hashlib.sha256(Network.store[("ghcr.io", tag_path)]).hexdigest()
(root / "result.json").write_text(json.dumps(result))
