#!/usr/bin/env python3
"""Publish one verified job image candidate to the fixed GHCR repository."""

from __future__ import annotations

import argparse
import base64
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import tomllib
from typing import Any, BinaryIO
from urllib.parse import parse_qsl, unquote, urlencode, urljoin, urlsplit, urlunsplit

import candidate

REPOSITORY = "tailrocks/velnor-new"
REPOSITORY_ID = 1390620900
REGISTRY = "ghcr.io"
PACKAGE = "tailrocks/velnor-job"
WORKFLOW = "tailrocks/velnor-new/.github/workflows/job-image-candidate.yml"
ARTIFACT_PREFIX = "velnor-job-image-candidate-"
MAX_HTTP_BODY = 2 * 1024 * 1024
MAX_RESULT_BYTES = 64 * 1024
MAX_UPLOAD_BYTES = candidate.MAX_BLOB_BYTES
CHUNK_BYTES = 1024 * 1024
TIMEOUT_SECONDS = 30


class PublishError(ValueError):
    """Publication evidence or registry response is invalid."""


def _fail(message: str) -> None:
    raise PublishError(message)


def _json(data: bytes, label: str) -> Any:
    try:
        return json.loads(data, object_pairs_hook=candidate._unique_object)
    except (UnicodeDecodeError, json.JSONDecodeError, candidate.CandidateError):
        _fail(f"{label} is invalid.")


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _sha_file(path: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as stream:
        while chunk := stream.read(CHUNK_BYTES):
            size += len(chunk)
            if size > candidate.MAX_ARCHIVE_BYTES:
                _fail("Candidate archive exceeds its size limit.")
            digest.update(chunk)
    return digest.hexdigest(), size


def _run_id(value: str, label: str) -> int:
    if not re.fullmatch(r"[1-9][0-9]{0,19}", value):
        _fail(f"{label} is invalid.")
    return int(value)


def _sha1(value: str, label: str) -> str:
    if not re.fullmatch(r"[0-9a-f]{40}", value):
        _fail(f"{label} is invalid.")
    return value


def _gh(args: list[str], timeout: int = TIMEOUT_SECONDS) -> bytes:
    env = os.environ.copy()
    env.pop("GH_HOST", None)
    try:
        return subprocess.check_output(
            ["gh", *args], env=env, stderr=subprocess.DEVNULL, timeout=timeout
        )
    except (OSError, subprocess.SubprocessError):
        _fail("GitHub CLI request failed.")


def _api(path: str) -> Any:
    raw = _gh(["api", path])
    if len(raw) > MAX_HTTP_BODY:
        _fail("GitHub API response exceeds its size limit.")
    return _json(raw, "GitHub API response")


def _current_identity(root: Path) -> dict[str, Any]:
    env = os.environ
    required = (
        "GITHUB_REPOSITORY", "GITHUB_REPOSITORY_ID", "GITHUB_REF",
        "GITHUB_EVENT_NAME", "GITHUB_WORKFLOW_REF", "GITHUB_SHA",
        "GITHUB_WORKFLOW_SHA", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT",
        "GH_TOKEN", "GITHUB_ACTOR", "VELNOR_CANDIDATE_ARTIFACT_ID",
    )
    if any(not env.get(name) for name in required):
        _fail("Required GitHub workflow identity is missing.")
    if env["GITHUB_REPOSITORY"] != REPOSITORY:
        _fail("Publication repository is invalid.")
    if env["GITHUB_REF"] != "refs/heads/main" or env["GITHUB_EVENT_NAME"] != "workflow_dispatch":
        _fail("Publication requires workflow_dispatch on refs/heads/main.")
    if env["GITHUB_WORKFLOW_REF"] != f"{WORKFLOW}@refs/heads/main":
        _fail("Publication workflow authority is invalid.")
    source = _sha1(env["GITHUB_SHA"], "Source SHA")
    authority = _sha1(env["GITHUB_WORKFLOW_SHA"], "Workflow authority SHA")
    if authority != source:
        _fail("Workflow authority SHA differs from source SHA.")
    if not env["GITHUB_REPOSITORY_ID"].isdigit() or int(env["GITHUB_REPOSITORY_ID"]) != REPOSITORY_ID:
        _fail("GitHub repository ID is invalid.")
    try:
        checkout = subprocess.check_output(
            ["git", "-C", str(root), "rev-parse", "HEAD"],
            stderr=subprocess.DEVNULL, timeout=TIMEOUT_SECONDS, text=True,
        ).strip()
    except (OSError, subprocess.SubprocessError):
        _fail("Cannot read checked-out source commit.")
    if checkout != source:
        _fail("Checked-out source differs from GitHub source SHA.")
    return {
        "repository": REPOSITORY,
        "repository_id": int(env["GITHUB_REPOSITORY_ID"]),
        "event": "workflow_dispatch",
        "workflow_ref": env["GITHUB_WORKFLOW_REF"],
        "ref": "refs/heads/main",
        "authority_commit": authority,
        "run_id": _run_id(env["GITHUB_RUN_ID"], "Publication run ID"),
        "run_attempt": _run_id(env["GITHUB_RUN_ATTEMPT"], "Publication run attempt"),
        "source": source,
        "artifact_id": _run_id(env["VELNOR_CANDIDATE_ARTIFACT_ID"], "Candidate artifact ID"),
    }


def _assert_current_main(identity: dict[str, Any]) -> None:
    repository = _api(f"repos/{REPOSITORY}")
    if (not isinstance(repository, dict) or repository.get("full_name") != REPOSITORY
            or type(repository.get("id")) is not int
            or repository["id"] != REPOSITORY_ID
            or identity["repository_id"] != REPOSITORY_ID):
        _fail("GitHub repository ID does not match the canonical repository.")
    tip = _api(f"repos/{REPOSITORY}/commits/main")
    if not isinstance(tip, dict) or tip.get("sha") != identity["source"]:
        _fail("Source SHA is not the current main commit.")


def _check_candidate(root: Path, archive: Path, record_path: Path,
                     identity: dict[str, Any]) -> tuple[dict[str, Any], bytes, dict[str, Any]]:
    if archive.is_symlink() or record_path.is_symlink() or not archive.is_file() or not record_path.is_file():
        _fail("Candidate inputs must be regular files.")
    record_raw = candidate._read_limited_file(record_path, candidate.MAX_RECORD_BYTES, "Candidate record")
    record = candidate.validate_record(candidate._json_bytes(record_raw, "candidate record"))
    oci = candidate.verify_oci_archive(archive)
    archive_hash, archive_size = _sha_file(archive)
    if record["archive"] != {"sha256": archive_hash, "size_bytes": archive_size}:
        _fail("Candidate archive does not match its signed record.")
    if record["oci"] != oci:
        _fail("Candidate OCI graph does not match its signed record.")
    candidate._verify_source_inputs(record, root)
    try:
        lock = tomllib.loads((root / ".velnor/runner.lock").read_text())
    except (OSError, tomllib.TOMLDecodeError):
        _fail("Runner image lock is invalid.")
    if not isinstance(lock, dict) or not isinstance(lock.get("buildx_version"), str):
        _fail("Runner image lock is invalid.")
    candidate._locked_buildx_identity(record["build"]["builder"], lock["buildx_version"])
    _check_candidate_identity(record, identity)
    return record, record_raw, oci


def _check_candidate_identity(record: dict[str, Any], identity: dict[str, Any]) -> None:
    if record["source"]["commit"] != identity["source"] or record["source"]["ref"] != "refs/heads/main":
        _fail("Candidate source is not the current main source.")
    if record["workflow"] != {
        "repository": REPOSITORY,
        "repository_id": identity["repository_id"],
        "event": "workflow_dispatch",
        "workflow_ref": identity["workflow_ref"],
        "authority_commit": identity["source"],
        "run_id": identity["run_id"],
        "run_attempt": identity["run_attempt"],
    }:
        _fail("Candidate run identity differs from the publication run.")


def _check_artifact(identity: dict[str, Any]) -> None:
    response = _api(f"repos/{REPOSITORY}/actions/runs/{identity['run_id']}/artifacts?per_page=100")
    if not isinstance(response, dict) or not isinstance(response.get("artifacts"), list):
        _fail("Candidate artifact response is invalid.")
    expected_name = f"{ARTIFACT_PREFIX}{identity['run_attempt']}"
    matches = [item for item in response["artifacts"] if isinstance(item, dict)
               and item.get("name") == expected_name]
    if len(matches) != 1:
        _fail("Expected one candidate artifact in this workflow run.")
    artifact = matches[0]
    if (artifact.get("id") != identity["artifact_id"] or artifact.get("expired") is not False
            or artifact.get("workflow_run", {}).get("id") != identity["run_id"]
            or artifact.get("workflow_run", {}).get("head_sha") != identity["source"]
            or artifact.get("workflow_run", {}).get("head_branch") != "main"):
        _fail("Candidate artifact ID, expiry, or workflow run does not match.")


def _check_ci(source: str) -> dict[str, Any]:
    response = _api(
        f"repos/{REPOSITORY}/actions/workflows/ci.yml/runs?head_sha={source}&branch=main&event=push&per_page=100"
    )
    if not isinstance(response, dict) or not isinstance(response.get("workflow_runs"), list):
        _fail("Canonical CI run response is invalid.")
    runs = [run for run in response["workflow_runs"] if isinstance(run, dict)
            and run.get("path") == ".github/workflows/ci.yml"
            and run.get("event") == "push" and run.get("head_branch") == "main"
            and run.get("head_sha") == source
            and isinstance(run.get("head_repository"), dict)
            and run["head_repository"].get("full_name") == REPOSITORY
            and type(run.get("run_number")) is int and type(run.get("run_attempt")) is int]
    if not runs:
        _fail("No canonical main CI push run exists for this source.")
    runs.sort(key=lambda item: (item["run_number"], item["id"]))
    latest = runs[-1]
    same_number = [item for item in runs if item["run_number"] == latest["run_number"]]
    if len(same_number) != 1 or latest.get("status") != "completed" or latest.get("conclusion") != "success":
        _fail("Latest canonical main CI run did not succeed.")
    return {"run_id": latest["id"], "run_attempt": latest["run_attempt"]}


def _verify_attestation(path: Path, source: str) -> None:
    args = [
        "attestation", "verify", str(path), "--repo", REPOSITORY,
        "--source-ref", "refs/heads/main", "--source-digest", source,
        "--signer-workflow", WORKFLOW, "--signer-digest", source,
    ]
    args.extend(["--format", "json"])
    raw = _gh(args, timeout=60)
    if len(raw) > MAX_HTTP_BODY:
        _fail("Attestation verification output exceeds its size limit.")
    verified = _json(raw, "Attestation verification output")
    if not isinstance(verified, list) or not verified:
        _fail("Attestation verification output has no verified records.")
    for item in verified:
        try:
            certificate = item["verificationResult"]["signature"]["certificate"]
            identifier = certificate["sourceRepositoryIdentifier"]
        except (KeyError, TypeError):
            _fail("Attestation certificate has no repository identifier.")
        if not ((type(identifier) is int and identifier == REPOSITORY_ID)
                or (type(identifier) is str and identifier == str(REPOSITORY_ID))):
            _fail("Attestation certificate repository ID is invalid.")
        if (certificate.get("githubWorkflowRepository") != REPOSITORY
                or certificate.get("githubWorkflowSHA") != source
                or certificate.get("githubWorkflowRef") != "refs/heads/main"
                or certificate.get("githubWorkflowTrigger") != "workflow_dispatch"
                or certificate.get("sourceRepositoryRef") != "refs/heads/main"
                or certificate.get("sourceRepositoryDigest") != source):
            _fail("Attestation certificate source or workflow identity is invalid.")


def _file_blob(archive: Path, digest: str, expected_size: int, directory: Path) -> Path:
    hexdigest = digest.removeprefix("sha256:")
    output = directory / hexdigest
    hasher = hashlib.sha256()
    size = 0
    try:
        with tarfile.open(archive, "r:") as tar:
            member = tar.getmember(f"blobs/sha256/{hexdigest}")
            if not member.isfile() or member.size != expected_size or expected_size > MAX_UPLOAD_BYTES:
                _fail("OCI blob descriptor size is invalid.")
            source = tar.extractfile(member)
            if source is None:
                _fail("OCI blob is missing from archive.")
            with source, output.open("xb") as destination:
                while chunk := source.read(CHUNK_BYTES):
                    size += len(chunk)
                    if size > expected_size:
                        _fail("OCI blob exceeds descriptor size.")
                    hasher.update(chunk)
                    destination.write(chunk)
    except (OSError, tarfile.TarError):
        _fail("Cannot read OCI blob from archive.")
    if size != expected_size or hasher.hexdigest() != hexdigest:
        _fail("OCI blob bytes do not match their descriptor.")
    return output


class _Response:
    def __init__(self, status: int, headers: dict[str, str], body: bytes):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self.body = body


class _HttpsTransport:
    """HTTPS-only transport; no redirects and bounded response reads."""

    def request(self, method: str, url: str, headers: dict[str, str],
                body: bytes | BinaryIO | None = None, length: int = 0) -> _Response:
        parts = urlsplit(url)
        if (parts.scheme != "https" or parts.hostname != REGISTRY or parts.port not in (None, 443)
                or parts.username is not None or parts.password is not None or parts.fragment):
            _fail("Registry URL is outside the fixed HTTPS origin.")
        path = urlunsplit(("", "", parts.path, parts.query, ""))
        connection = http.client.HTTPSConnection(REGISTRY, timeout=TIMEOUT_SECONDS)
        try:
            connection.putrequest(method, path)
            for name, value in headers.items():
                if "\r" in name + value or "\n" in name + value:
                    _fail("Invalid registry request header.")
                connection.putheader(name, value)
            if body is not None:
                connection.putheader("Content-Length", str(length))
            connection.endheaders()
            if body is not None:
                remaining = length
                if isinstance(body, bytes):
                    if len(body) != length:
                        _fail("Registry request body length changed.")
                    connection.send(body)
                else:
                    while remaining:
                        chunk = body.read(min(CHUNK_BYTES, remaining))
                        if not chunk:
                            _fail("Registry request body ended early.")
                        connection.send(chunk)
                        remaining -= len(chunk)
                    if body.read(1):
                        _fail("Registry request body exceeds its declared length.")
            response = connection.getresponse()
            chunks = bytearray()
            while True:
                chunk = response.read(min(CHUNK_BYTES, MAX_HTTP_BODY + 1 - len(chunks)))
                if not chunk:
                    break
                chunks.extend(chunk)
                if len(chunks) > MAX_HTTP_BODY:
                    _fail("Registry response exceeds its size limit.")
            return _Response(response.status, dict(response.getheaders()), bytes(chunks))
        except (OSError, http.client.HTTPException, TimeoutError):
            _fail("Registry request failed or timed out.")
        finally:
            connection.close()

    def verify_get(self, url: str, headers: dict[str, str], expected: bytes | Path,
                   size: int, digest: str) -> _Response:
        parts = urlsplit(url)
        if (parts.scheme != "https" or parts.hostname != REGISTRY or parts.port not in (None, 443)
                or parts.username is not None or parts.password is not None or parts.fragment):
            _fail("Registry URL is outside the fixed HTTPS origin.")
        connection = http.client.HTTPSConnection(REGISTRY, timeout=TIMEOUT_SECONDS)
        try:
            path = urlunsplit(("", "", parts.path, parts.query, ""))
            connection.request("GET", path, headers=headers)
            response = connection.getresponse()
            response_headers = dict(response.getheaders())
            normalized = {key.lower(): value for key, value in response_headers.items()}
            if response.status != 200 or normalized.get("content-length") != str(size):
                _fail("Registry GET verification failed.")
            if normalized.get("docker-content-digest") not in (None, digest):
                _fail("Registry GET digest header differs from the requested digest.")
            hasher = hashlib.sha256()
            remaining = size
            if isinstance(expected, bytes):
                offset = 0
                while remaining:
                    chunk = response.read(min(CHUNK_BYTES, remaining))
                    if not chunk or chunk != expected[offset:offset + len(chunk)]:
                        _fail("Registry manifest bytes changed during publication.")
                    offset += len(chunk)
                    remaining -= len(chunk)
                    hasher.update(chunk)
            else:
                with expected.open("rb") as stream:
                    while remaining:
                        chunk = response.read(min(CHUNK_BYTES, remaining))
                        expected_chunk = stream.read(len(chunk)) if chunk else b""
                        if not chunk or chunk != expected_chunk:
                            _fail("Registry blob bytes changed during publication.")
                        remaining -= len(chunk)
                        hasher.update(chunk)
                    if stream.read(1):
                        _fail("Expected OCI blob changed during verification.")
            if response.read(1) or hasher.hexdigest() != digest.removeprefix("sha256:"):
                _fail("Registry returned bytes with the wrong digest or size.")
            return _Response(response.status, response_headers, b"")
        except (OSError, http.client.HTTPException, TimeoutError):
            _fail("Registry GET request failed or timed out.")
        finally:
            connection.close()


def _auth_token(transport: _HttpsTransport, actor: str, github_token: str) -> str:
    challenge = transport.request("GET", f"https://{REGISTRY}/v2/", {})
    if challenge.status != 401:
        _fail("Registry did not require authentication.")
    value = challenge.headers.get("www-authenticate", "")
    match = re.fullmatch(r"Bearer\s+(.+)", value)
    if match is None:
        _fail("Registry authentication challenge is invalid.")
    params: dict[str, str] = {}
    remaining = match.group(1)
    while remaining:
        part = re.match(r'\s*([A-Za-z][A-Za-z0-9_-]*)="([^"\\]*)"\s*(?:,|$)', remaining)
        if part is None or part.group(1) in params:
            _fail("Registry authentication challenge is invalid.")
        params[part.group(1)] = part.group(2)
        remaining = remaining[part.end():]
    if (set(params) - {"realm", "service", "scope"}
            or params.get("realm") != f"https://{REGISTRY}/token"
            or params.get("service") != REGISTRY):
        _fail("Registry authentication challenge is invalid.")
    basic = base64.b64encode(f"{actor}:{github_token}".encode()).decode("ascii")
    query = urlencode({"service": "ghcr.io", "scope": f"repository:{PACKAGE}:pull,push"})
    response = transport.request(
        "GET", f"https://{REGISTRY}/token?{query}",
        {"Authorization": f"Basic {basic}", "Accept": "application/json"}
    )
    if response.status != 200:
        _fail("Registry token request failed.")
    data = _json(response.body, "Registry token response")
    token = data.get("token", data.get("access_token")) if isinstance(data, dict) else None
    if not isinstance(token, str) or not re.fullmatch(r"[A-Za-z0-9._~=-]{16,8192}", token):
        _fail("Registry token response has no valid token.")
    return token


def _registry_url(path: str) -> str:
    return f"https://{REGISTRY}/v2/{PACKAGE}/{path}"


def _request(transport: _HttpsTransport, token: str, method: str, url: str,
             headers: dict[str, str] | None = None, body: bytes | BinaryIO | None = None,
             length: int = 0) -> _Response:
    request_headers = {"Authorization": f"Bearer {token}", **(headers or {})}
    result = transport.request(method, url, request_headers, body, length)
    if 300 <= result.status < 400:
        _fail("Registry redirects are not accepted.")
    return result


def _validate_location(base: str, location: str) -> str:
    if not location or len(location) > 4096 or "\r" in location or "\n" in location or "\\" in location:
        _fail("Registry upload location is missing or too long.")
    resolved = urljoin(base, location)
    parsed = urlsplit(resolved)
    expected_prefix = f"/v2/{PACKAGE}/blobs/uploads/"
    decoded_path = unquote(parsed.path)
    if (parsed.scheme != "https" or parsed.hostname != REGISTRY or parsed.port not in (None, 443)
            or parsed.username is not None or parsed.password is not None or parsed.fragment
            or not decoded_path.startswith(expected_prefix)
            or any(part in {".", ".."} for part in decoded_path.split("/"))):
        _fail("Registry upload location is outside the fixed HTTPS upload path.")
    if any(not key or "\r" in key + value or "\n" in key + value
           for key, value in parse_qsl(parsed.query, keep_blank_values=True)):
        _fail("Registry upload query is invalid.")
    return resolved


def _head_object(transport: _HttpsTransport, token: str, kind: str, digest: str,
                 size: int) -> bool:
    media = "application/vnd.oci.image.manifest.v1+json" if kind == "manifests" else None
    headers = {"Accept": media} if media else {}
    response = _request(transport, token, "HEAD", _registry_url(f"{kind}/{digest}"), headers)
    if response.status == 404:
        return False
    if response.status != 200 or response.headers.get("docker-content-digest") != digest:
        _fail("Registry digest HEAD verification failed.")
    if response.headers.get("content-length") != str(size):
        _fail("Registry size HEAD verification failed.")
    return True


def _verify_get(transport: _HttpsTransport, token: str, kind: str, digest: str,
                expected: bytes | Path, size: int) -> None:
    media = "application/vnd.oci.image.manifest.v1+json" if kind == "manifests" else "application/octet-stream"
    _request_stream = getattr(transport, "verify_get", None)
    if _request_stream is None:
        response = _request(transport, token, "GET", _registry_url(f"{kind}/{digest}"),
                            {"Accept": media})
        if response.status != 200 or len(response.body) != size:
            _fail("Registry GET verification failed.")
        if response.headers.get("docker-content-digest") not in (None, digest):
            _fail("Registry GET digest header differs from the requested digest.")
        if hashlib.sha256(response.body).hexdigest() != digest.removeprefix("sha256:"):
            _fail("Registry returned bytes with the wrong digest.")
        if isinstance(expected, bytes) and response.body != expected:
            _fail("Registry manifest bytes changed during publication.")
        if isinstance(expected, Path):
            with expected.open("rb") as stream:
                if stream.read(size + 1) != response.body:
                    _fail("Registry blob bytes changed during publication.")
        return
    headers = {"Authorization": f"Bearer {token}", "Accept": media}
    _request_stream(_registry_url(f"{kind}/{digest}"), headers, expected, size, digest)


def _upload_blob(transport: _HttpsTransport, token: str, digest: str, path: Path) -> None:
    size = path.stat().st_size
    if _head_object(transport, token, "blobs", digest, size):
        return
    start_url = _registry_url("blobs/uploads/")
    started = _request(transport, token, "POST", start_url, {"Content-Length": "0"})
    if started.status != 202 or "location" not in started.headers:
        _fail("Registry upload session could not start.")
    location = _validate_location(start_url, started.headers["location"])
    parts = urlsplit(location)
    query = parse_qsl(parts.query, keep_blank_values=True)
    if any(key == "digest" for key, _ in query):
        _fail("Registry upload location contains a digest parameter.")
    digest_query = urlencode({"digest": digest})
    merged_query = f"{parts.query}&{digest_query}" if parts.query else digest_query
    upload_url = urlunsplit((parts.scheme, parts.netloc, parts.path, merged_query, ""))
    try:
        with path.open("rb") as stream:
            response = _request(
                transport, token, "PUT", upload_url,
                {"Content-Type": "application/octet-stream"}, stream, size,
            )
    except PublishError:
        if _head_object(transport, token, "blobs", digest, size):
            return
        raise
    if response.status != 201:
        _fail("Registry blob upload did not complete.")
    if not _head_object(transport, token, "blobs", digest, size):
        _fail("Registry blob is absent after upload.")


def _upload_manifest(transport: _HttpsTransport, token: str, manifest: bytes,
                    digest: str) -> None:
    size = len(manifest)
    if _head_object(transport, token, "manifests", digest, size):
        _verify_get(transport, token, "manifests", digest, manifest, size)
        return
    headers = {"Content-Type": "application/vnd.oci.image.manifest.v1+json"}
    try:
        response = _request(
            transport, token, "PUT", _registry_url(f"manifests/{digest}"), headers, manifest, size
        )
    except PublishError:
        if _head_object(transport, token, "manifests", digest, size):
            _verify_get(transport, token, "manifests", digest, manifest, size)
            return
        raise
    if response.status != 201:
        _fail("Registry manifest upload did not complete.")
    if response.headers.get("docker-content-digest") not in (None, digest):
        _fail("Registry reported a different manifest digest.")
    if not _head_object(transport, token, "manifests", digest, size):
        _fail("Registry manifest is absent after upload.")
    _verify_get(transport, token, "manifests", digest, manifest, size)


def _read_manifest(archive: Path, digest: str, size: int) -> bytes:
    if size > MAX_HTTP_BODY:
        _fail("OCI manifest exceeds its size limit.")
    try:
        raw = candidate._read_blob(archive, digest.removeprefix("sha256:"))
    except (candidate.CandidateError, KeyError, OSError, tarfile.TarError):
        _fail("Cannot read OCI manifest bytes.")
    if len(raw) != size or _sha(raw) != digest.removeprefix("sha256:"):
        _fail("OCI platform manifest bytes changed after validation.")
    return raw


def _blob_descriptors(manifest: dict[str, Any]) -> dict[str, int]:
    config = manifest.get("config")
    layers = manifest.get("layers")
    if not isinstance(config, dict) or not isinstance(layers, list):
        _fail("OCI manifest blob descriptors are invalid.")
    unique: dict[str, int] = {}
    for descriptor in [config, *layers]:
        if not isinstance(descriptor, dict):
            _fail("OCI blob descriptor is invalid.")
        digest = descriptor.get("digest")
        size = descriptor.get("size")
        if (not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest)
                or type(size) is not int or not 1 <= size <= MAX_UPLOAD_BYTES):
            _fail("OCI blob descriptor is invalid.")
        if digest in unique and unique[digest] != size:
            _fail("Repeated OCI blob digest has inconsistent sizes.")
        unique[digest] = size
    return unique


def publish(args: argparse.Namespace) -> None:
    root = Path(args.root).resolve(strict=True)
    archive_input = Path(args.archive)
    record_input = Path(args.record)
    if archive_input.is_symlink() or record_input.is_symlink():
        _fail("Candidate inputs must be regular files.")
    archive = archive_input.resolve(strict=True)
    record_path = record_input.resolve(strict=True)
    result_path = Path(args.result).absolute()
    if result_path.is_symlink() or result_path.exists() or root == result_path or root in result_path.parents:
        _fail("Result path must be new and outside the source tree.")
    identity = _current_identity(root)
    _assert_current_main(identity)
    record, record_raw, oci = _check_candidate(root, archive, record_path, identity)
    _check_artifact(identity)
    ci = _check_ci(identity["source"])
    _verify_attestation(archive, identity["source"])
    _verify_attestation(record_path, identity["source"])
    _assert_current_main(identity)

    platform_manifest_digest = oci["platform_manifest_digest"]
    platform_manifest = _read_manifest(archive, platform_manifest_digest,
                                       oci["platform_manifest_size_bytes"])
    manifest_value = _json(platform_manifest, "OCI manifest")
    if not isinstance(manifest_value, dict):
        _fail("OCI platform manifest is invalid.")
    with tempfile.TemporaryDirectory(prefix="velnor-job-publish-") as temp:
        temp_path = Path(temp)
        config = manifest_value["config"]
        layers = manifest_value["layers"]
        unique = _blob_descriptors(manifest_value)
        blob_paths: dict[str, Path] = {}
        for digest, size in unique.items():
            blob_paths[digest] = _file_blob(archive, digest, size, temp_path)
        transport = _HttpsTransport()
        token = _auth_token(transport, os.environ["GITHUB_ACTOR"], os.environ["GH_TOKEN"])
        for digest, path in blob_paths.items():
            _upload_blob(transport, token, digest, path)
        _upload_manifest(transport, token, platform_manifest, platform_manifest_digest)
        for digest, path in blob_paths.items():
            if not _head_object(transport, token, "blobs", digest, path.stat().st_size):
                _fail("Registry blob is absent after manifest upload.")
            _verify_get(transport, token, "blobs", digest, path, path.stat().st_size)

    _assert_current_main(identity)
    evidence = {
        "schema": "velnor.job-image-publication.v1",
        "status": "PASS",
        "package": f"{REGISTRY}/{PACKAGE}",
        "digest_reference": f"{REGISTRY}/{PACKAGE}@{platform_manifest_digest}",
        "private_package_access_gate": "not_proven",
        "archive": {"sha256": record["archive"]["sha256"], "size_bytes": record["archive"]["size_bytes"]},
        "source": identity["source"],
        "workflow": {"ref": identity["workflow_ref"], "authority_sha": identity["source"],
                     "run_id": identity["run_id"], "run_attempt": identity["run_attempt"],
                     "artifact_id": identity["artifact_id"]},
        "ci": ci,
        "oci": {
            "platform": "linux/amd64",
            "archive_index_digest": "sha256:" + oci["archive_index_digest"],
            "manifest_digest": platform_manifest_digest,
            "manifest_size_bytes": len(platform_manifest),
            "config_digest": manifest_value["config"]["digest"],
            "layer_digests": [layer["digest"] for layer in layers],
        },
        "record_sha256": _sha(record_raw),
    }
    encoded = (json.dumps(evidence, sort_keys=True, separators=(",", ":")) + "\n").encode()
    if len(encoded) > MAX_RESULT_BYTES:
        _fail("Publication evidence exceeds its size limit.")
    result_path.parent.mkdir(parents=True, exist_ok=True)
    result_path.write_bytes(encoded)
    print("PASS: Verified job image published by OCI digest; private package access remains unproven.")


def main() -> None:
    parser = argparse.ArgumentParser(description="Verify and publish a job image candidate.")
    parser.add_argument("--root", required=True)
    parser.add_argument("--archive", required=True)
    parser.add_argument("--record", required=True)
    parser.add_argument("--result", required=True)
    args = parser.parse_args()
    try:
        publish(args)
    except candidate.CandidateError:
        print("FAIL: Candidate validation failed.", file=__import__("sys").stderr)
        raise SystemExit(1) from None
    except PublishError as error:
        print(f"FAIL: {error}", file=__import__("sys").stderr)
        raise SystemExit(1) from None


if __name__ == "__main__":
    main()
