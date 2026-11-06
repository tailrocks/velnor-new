// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Thread 4389237067: stream OCI blobs instead of retaining them all.

use super::*;

pub(crate) const DRIVER: &str = r#"import hashlib
import json
import sys
import tempfile
from pathlib import Path
from urllib.parse import parse_qsl, urlsplit

ROOT = Path(sys.argv[1])

sys.path.insert(0, str(ROOT))

import job_image_authority_oci as oci_module
import job_image_authority_publish as publish_module
import job_image_authority_record as record_module
import job_image_authority_registry as registry_module

ERRORS = (
    oci_module.AuthorityError,
    publish_module.AuthorityError,
    record_module.AuthorityError,
    registry_module.AuthorityError,
)


class _FakeResponse:
    def __init__(self, status, headers, body):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self.body = body


class _FakeStream:
    """Chunked readback; the registry response is never buffered at once."""

    def __init__(self, status, headers, data):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self._data = data
        self._offset = 0

    def read(self, size=-1):
        if size is None or size < 0:
            size = len(self._data) - self._offset
        chunk = self._data[self._offset:self._offset + size]
        self._offset += len(chunk)
        return chunk

    def close(self):
        pass


class _FakeRegistry:
    """Streaming fake: blob uploads must arrive as temp-file paths."""

    def __init__(self, tamper_get=False):
        self.store = {}
        self.puts = 0
        self.tamper_get = tamper_get
        self.blob_put_buffered = False

    def _key(self, path):
        digest = path.rsplit("/", 1)[-1]
        kind = "manifests" if "/manifests/" in path else "blobs"
        return f"{kind}:{digest}", digest

    def request(self, method, url, headers, body=None, length=0):
        parts = urlsplit(url)
        path = parts.path
        if method == "GET" and path == "/v2/":
            return _FakeResponse(
                401,
                {
                    "www-authenticate": 'Bearer realm="https://ghcr.io/token",'
                    'service="ghcr.io",scope="repository:tailrocks/velnor-job:pull,push"'
                },
                b"",
            )
        if method == "GET" and path == "/token":
            return _FakeResponse(200, {}, b'{"token": "stream-token-0123456789abcdef"}')
        if method == "HEAD":
            key, digest = self._key(path)
            if key in self.store:
                data = self.store[key]
                return _FakeResponse(
                    200,
                    {
                        "docker-content-digest": digest,
                        "content-length": str(len(data)),
                    },
                    b"",
                )
            return _FakeResponse(404, {}, b"")
        if method == "POST":
            return _FakeResponse(
                202,
                {"location": "/v2/tailrocks/velnor-job/blobs/uploads/stream-uuid"},
                b"",
            )
        if method == "PUT":
            query = dict(parse_qsl(parts.query))
            if "/manifests/" in path:
                key, digest = self._key(path)
                if not isinstance(body, bytes) or len(body) != length:
                    raise AssertionError("manifest PUT must send exact bytes")
                self.store[key] = body
                self.puts += 1
                return _FakeResponse(201, {"docker-content-digest": digest}, b"")
            if isinstance(body, (bytes, bytearray, memoryview)):
                self.blob_put_buffered = True
                data = bytes(body)
                if len(data) != length:
                    raise AssertionError("buffered blob PUT length changed")
            else:
                chunks = []
                with open(body, "rb") as stream:
                    while chunk := stream.read(65536):
                        chunks.append(chunk)
                data = b"".join(chunks)
                if len(data) != length:
                    raise AssertionError("streamed blob PUT length changed")
            self.store[f"blobs:{query['digest']}"] = data
            self.puts += 1
            return _FakeResponse(201, {}, b"")
        if method == "GET":
            raise AssertionError("blob and manifest GET must use the streaming call")
        raise AssertionError(f"unexpected registry call: {method} {url}")

    def stream(self, method, url, headers):
        if method != "GET":
            raise AssertionError(f"unexpected stream call: {method} {url}")
        parts = urlsplit(url)
        path = parts.path
        key, digest = self._key(path)
        stored = self.store[key]
        data = stored
        if self.tamper_get and "/manifests/" not in path:
            data = b"X" + stored[1:]
        return _FakeStream(
            200,
            {"docker-content-digest": digest, "content-length": str(len(stored))},
            data,
        )


def run_stream_publish(tamper):
    try:
        record_path = Path(sys.argv[3])
        archive_path = Path(sys.argv[4])
        params = json.loads(Path(sys.argv[5]).read_text())
        record = record_module.validate_record(record_module.load_record(record_path))
        verified = oci_module.verify_oci_archive(archive_path)
        hasher = hashlib.sha256()
        archive_size = 0
        with archive_path.open("rb") as archive_stream:
            while chunk := archive_stream.read(1048576):
                hasher.update(chunk)
                archive_size += len(chunk)
        publish_module.check_record_binding(
            record, hasher.hexdigest(), archive_size, verified
        )
        manifest = publish_module.read_manifest_bytes(
            archive_path,
            verified["platform_manifest_digest"],
            verified["platform_manifest_size_bytes"],
        )
        manifest_value = json.loads(manifest)
        registry = _FakeRegistry(tamper_get=tamper)
        with tempfile.TemporaryDirectory(prefix="gc1-t51-stream-") as temp:
            blob_paths = {}
            for digest, size in publish_module.blob_descriptors(manifest_value).items():
                blob_paths[digest] = publish_module.extract_blob(
                    archive_path, digest, size, Path(temp)
                )
            token = publish_module.fetch_registry_token(
                registry_module, registry, "actor", "password-value"
            )
            proof = publish_module.publish_image(
                registry_module, registry, token, manifest, blob_paths
            )
        if registry.blob_put_buffered:
            print("PUT-BUFFERED")
        else:
            print("PUT-STREAMED")
        record_raw = record_path.read_bytes()
        evidence = publish_module.build_evidence(
            params["identity"],
            params["ci"],
            {"sha256": hasher.hexdigest(), "size_bytes": archive_size},
            verified,
            manifest,
            hashlib.sha256(record_raw).hexdigest(),
            params["record_source"],
            proof,
        )
        (ROOT / "evidence.json").write_bytes(evidence)
        publish_module.validate_evidence(json.loads(evidence))
    except ERRORS as error:
        print(f"STREAM-FLOW-REJECTED {error}")
        return
    print("STREAM-FLOW-OK")
    print("digest_reference=" + json.loads(evidence)["digest_reference"])


def run_bytes_rejected():
    try:
        body = b"blob-body"
        digest = "sha256:" + hashlib.sha256(body).hexdigest()
        manifest = (
            '{"schemaVersion":2,"config":{"digest":"'
            + digest
            + '","size":9},"layers":[{"digest":"'
            + digest
            + '","size":9}]}'
        ).encode()
        publish_module.publish_image(
            registry_module,
            _FakeRegistry(),
            "test-token-0123456789abcdef",
            manifest,
            {digest: body},
        )
    except ERRORS as error:
        print(f"BYTES-REJECTED {error}")
        return
    raise SystemExit(3)


def run_check_evidence():
    try:
        value = json.loads(Path(sys.argv[3]).read_bytes())
        publish_module.validate_evidence(value)
    except ERRORS as error:
        print(f"EVIDENCE-REJECTED {error}")
        return
    print("EVIDENCE-VALID " + value["digest_reference"])


def main():
    mode = sys.argv[2]
    if mode == "stream_publish":
        run_stream_publish(tamper=False)
    elif mode == "stream_publish_tamper":
        run_stream_publish(tamper=True)
    elif mode == "bytes_rejected":
        run_bytes_rejected()
    elif mode == "check_evidence":
        run_check_evidence()
    else:
        raise SystemExit(2)


main()
"#;

pub(super) fn check_stream_contract() -> Result<(), Box<dyn Error>> {
    for (needle, label) in [
        (
            "def publish_image(registry, transport, token, manifest, blob_paths):",
            "streaming publish contract",
        ),
        (
            "OCI blob must be a temporary file path, not bytes.",
            "bytes contract rejection",
        ),
        (
            "def _open_stream(transport, method, url, headers):",
            "streaming open helper",
        ),
        ("def _read_exact(reader, count):", "short-read helper"),
        (
            "def verify_remote_blob(registry, transport, token, digest, path, size):",
            "streaming blob verification",
        ),
        (
            "def upload_blob(registry, transport, token, digest, path, size):",
            "streaming blob upload",
        ),
        (
            "transport.stream(method, url, headers)",
            "transport streaming call",
        ),
        (
            "def _require_blob_file(digest, path, size):",
            "path-only blob guard",
        ),
        ("stream.seek(0, 2)", "size probe without reading"),
    ] {
        require(PUBLISH_SOURCE, needle, label)?;
    }
    for (needle, label) in [
        ("digest, expected, size", "old bytes verification signature"),
        ("body=data", "old buffered blob upload"),
        ("def _remote_bytes", "old buffered remote read"),
        ("def _require_blob(digest, data):", "old bytes blob guard"),
        ("import os", "os import"),
        ("import stat", "stat import"),
    ] {
        forbid(PUBLISH_SOURCE, needle, label)?;
    }
    Ok(())
}

pub(super) fn check_evidence_streams() -> Result<(), Box<dyn Error>> {
    for (needle, label) in [
        (
            "def stream(self, method, url, headers):",
            "registry streaming call",
        ),
        ("RESPONSE_BODY_LIMIT = 65536", "bounded control responses"),
        (
            "blob_paths[digest] = publish_module.extract_blob",
            "temp-file blob handles",
        ),
        (
            "publish_module.publish_image(registry_module, transport, token, manifest, blob_paths)",
            "streaming publication call",
        ),
    ] {
        require(AUTHORITY_MODULE, needle, label)?;
    }
    for (needle, label) in [
        (
            "limit = publish_module.MAX_UPLOAD_BYTES + 1",
            "gibibyte response buffer",
        ),
        (
            "extract_blob(archive_path, digest, size, Path(temp)).read_bytes()",
            "in-memory blob retention",
        ),
    ] {
        forbid(AUTHORITY_MODULE, needle, label)?;
    }
    Ok(())
}

#[path = "gc1_t51_stream_driver.rs"]
mod gc1_t51_stream_driver;
