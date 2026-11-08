# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import base64
import hashlib
import json
import sys
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

root = Path(sys.argv[1])
sys.path.insert(0, str(root))
import job_image_authority_transport as transport
import job_image_authority_registry as registry_module
import runner_image_authority_candidate as candidate
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
    def __init__(self, host, timeout=None, context=None):
        self.host = host
        self.headers = {}
        self.sent = bytearray()
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
        if self.method == "GET" and target == "/v2/":
            return Response(401, {"www-authenticate": 'Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:fixture:pull,push"'})
        if self.method == "GET" and target.startswith("/token"):
            return Response(200, {}, b'{"token":"stub-token-1234567890"}')
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
            data = self.store.get(key)
            return Response(200, {"docker-content-digest": "sha256:" + hashlib.sha256(data).hexdigest(), "content-length": str(len(data))}, data)
        raise AssertionError((self.method, target))

transport.http.client.HTTPSConnection = Network
transport.http.client.HTTPException = OSError
transports = {role: transport.RegistryTransport(30, publication.registry.RegistryPolicy(profile)) for role, profile in publication.PROFILES.items()}
inputs_for_roles = {role: {"archive": archive, "record": records[role].encode(), "envelope": envelopes[role].encode()} for role in records}
if scenario == "tampered_third_record":
    inputs_for_roles["resource_probe"]["record"] += b"\n"
tag_path = f"/v2/{publication.PROFILES['runner'].value}/manifests/sha-{source}"
preseed = b"fixed-conflicting-source-tag"
if scenario == "conflicting_source_tag":
    Network.store[("ghcr.io", tag_path)] = preseed
def current_source():
    runner_published = any(host == "ghcr.io" and path.startswith("/v2/tailrocks/velnor-runner/manifests/sha256:") for host, path in Network.store)
    if scenario == "stale_before_publish" or (scenario == "stale_after_image" and runner_published):
        return "c" * 40
    return source

try:
    manifest = publication.publish_family(inputs_for_roles, receipt_raw, json.dumps(verification).encode(), source, 101, 1, 202, 2, 303, lock_raw, root / "private", "actor", "password", transports, current_source)
    (root / "manifest.json").write_bytes(manifest)
    result = {"ok": True}
except (publication.AuthorityError, identity.AuthorityError, candidate.AuthorityError, registry_module.AuthorityError) as error:
    result = {"ok": False, "error": str(error)}
result["registry_puts"] = Network.puts
result["stored_paths"] = sorted(path for host, path in Network.store if host == "ghcr.io")
result["source_tags"] = {}
for role, profile in publication.PROFILES.items():
    target = f"/v2/{profile.value}/manifests/sha-{source}"
    body = Network.store.get(("ghcr.io", target))
    result["source_tags"][role] = None if body is None else hashlib.sha256(body).hexdigest()
if scenario == "conflicting_source_tag":
    result["preseed_sha256"] = hashlib.sha256(preseed).hexdigest()
    result["runner_tag_after_sha256"] = hashlib.sha256(Network.store[("ghcr.io", tag_path)]).hexdigest()
(root / "result.json").write_text(json.dumps(result))
