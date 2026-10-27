// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) const DRIVER: &str = r#"import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(sys.argv[1])

sys.path.insert(0, str(ROOT))

import job_image_authority_identity as identity_module
import job_image_authority_oci as oci_module
import job_image_authority_publish as publish_module
import job_image_authority_record as record_module
import job_image_authority_registry as registry_module
import job_image_authority_tar as tar_module

ERRORS = (
    identity_module.AuthorityError,
    oci_module.AuthorityError,
    publish_module.AuthorityError,
    record_module.AuthorityError,
    registry_module.AuthorityError,
    tar_module.AuthorityError,
)


def envelope():
    return json.loads(Path(sys.argv[3]).read_text())


def report(marker, detail):
    print(marker)
    if detail:
        print(detail)


def run_identity(case):
    try:
        identity_module.validate_identity(case)
    except ERRORS as error:
        report("IDENTITY-REJECTED", error)
        return
    report("IDENTITY-OK", case["source"])


def run_source_inputs(case):
    try:
        commit = identity_module.validate_source_inputs(case["record_source"], case["authenticated"])
    except ERRORS as error:
        report("SOURCE-INPUTS-REJECTED", error)
        return
    report("SOURCE-INPUTS-OK", commit)


def run_build_lock(case):
    try:
        identity_module.validate_build_lock(case["builder"], case["buildx_version"])
    except ERRORS as error:
        report("BUILD-LOCK-REJECTED", error)
        return
    report("BUILD-LOCK-OK", case["buildx_version"])


def run_redirect(case):
    try:
        target = registry_module.redirect_target(case["path"], case["digest"], case["status"], case["location"])
    except ERRORS as error:
        report("REDIRECT-REJECTED", error)
        return
    if target is None:
        report("REDIRECT-NONE", "direct")
    else:
        report("REDIRECT-OK", target)


def run_record_ok(path):
    record_module.validate_record(record_module.load_record(Path(path)))
    print("RECORD-OK")


def run_record_bad(path):
    try:
        record_module.validate_record(record_module.load_record(Path(path)))
    except ERRORS as error:
        print(f"RECORD-REJECTED {error}")
        return
    raise SystemExit(3)


def run_archive_ok(path):
    result = oci_module.verify_oci_archive(Path(path))
    print("ARCHIVE-OK")
    print(f"archive_index_digest={result['archive_index_digest']}")
    print(f"platform_manifest_digest={result['platform_manifest_digest']}")
    print(f"layer_digests={','.join(result['layer_digests'])}")


def run_archive_bad(path):
    try:
        oci_module.verify_oci_archive(Path(path))
    except ERRORS as error:
        print(f"ARCHIVE-REJECTED {error}")
        return
    raise SystemExit(3)


def run_tar_member(name):
    try:
        first = tar_module._safe_member(name)
    except ERRORS as error:
        try:
            oci_module._safe_member(name)
        except ERRORS as other:
            if str(other) != str(error):
                raise SystemExit(4)
            print(f"TAR-MEMBER-REJECTED {error}")
            return
        raise SystemExit(4)
    try:
        second = oci_module._safe_member(name)
    except ERRORS:
        raise SystemExit(4)
    if second != first:
        raise SystemExit(4)
    print(f"TAR-MEMBER-OK {first}")


def run_descriptors(case):
    try:
        unique = publish_module.blob_descriptors(case["manifest"])
    except ERRORS as error:
        report("DESCRIPTORS-REJECTED", error)
        return
    report("DESCRIPTORS-OK", f"digests={','.join(sorted(unique))}")


def run_manifest(case):
    try:
        raw = publish_module.read_manifest_bytes(Path(case["archive"]), case["digest"], case["size"])
    except ERRORS as error:
        report("MANIFEST-REJECTED", error)
        return
    report("MANIFEST-OK", f"size={len(raw)}")


def run_extract(case):
    try:
        with tempfile.TemporaryDirectory(prefix="gc1-t47-extract-") as temp:
            output = publish_module.extract_blob(
                Path(case["archive"]), case["digest"], case["size"], Path(temp)
            )
            data = Path(output).read_bytes()
    except ERRORS as error:
        report("EXTRACT-REJECTED", error)
        return
    import hashlib

    report("EXTRACT-OK", f"sha={hashlib.sha256(data).hexdigest()} size={len(data)}")


class _FakeResponse:
    def __init__(self, status, headers, body):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self.body = body


class _FakeStream:
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
    """In-memory OCI registry for publish-path tests; no network is opened."""

    def __init__(self):
        self.store = {}
        self.puts = 0

    def _key(self, path):
        digest = path.rsplit("/", 1)[-1]
        kind = "manifests" if "/manifests/" in path else "blobs"
        return f"{kind}:{digest}", digest

    def request(self, method, url, headers, body=None, length=0):
        from urllib.parse import parse_qsl, urlsplit

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
            import json as json_module

            return _FakeResponse(
                200, {}, json_module.dumps({"token": "t" * 32}).encode()
            )
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
                {"location": "/v2/tailrocks/velnor-job/blobs/uploads/fake-uuid"},
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
                raise AssertionError("blob PUT must stream a temp file, not bytes")
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
        from urllib.parse import urlsplit as _split

        if method != "GET":
            raise AssertionError(f"unexpected stream call: {method} {url}")
        path = _split(url).path
        key, digest = self._key(path)
        stored = self.store[key]
        return _FakeStream(
            200,
            {"docker-content-digest": digest, "content-length": str(len(stored))},
            stored,
        )


def run_evidence(case):
    try:
        manifest = bytes.fromhex(case["manifest_hex"])
        with tempfile.TemporaryDirectory(prefix="gc1-t47-evidencestream-") as temp:
            blob_paths = {}
            for digest, body_hex in case["blobs"].items():
                path = Path(temp) / digest.removeprefix("sha256:")
                path.write_bytes(bytes.fromhex(body_hex))
                blob_paths[digest] = path
            proof = publish_module.publish_image(
                registry_module,
                _FakeRegistry(),
                "test-token-0123456789abcdef",
                manifest,
                blob_paths,
            )
        encoded = publish_module.build_evidence(
            case["identity"],
            case["ci"],
            case["record_archive"],
            case["oci"],
            manifest,
            case["record_sha256"],
            case["record_source"],
            proof,
        )
    except ERRORS as error:
        report("EVIDENCE-REJECTED", error)
        return
    (ROOT / "evidence.json").write_bytes(encoded)
    report("EVIDENCE-OK", f"size={len(encoded)}")


def run_evidence_check(case):
    try:
        publish_module.validate_evidence(case["evidence"])
    except ERRORS as error:
        report("EVIDENCE-REJECTED", error)
        return
    report("EVIDENCE-VALID", case["evidence"]["digest_reference"])


def main():
    mode = sys.argv[2]
    if mode in (
        "identity", "source_inputs", "build_lock", "redirect", "descriptors",
        "manifest", "extract", "evidence", "evidence_check",
    ):
        {
            "identity": run_identity,
            "source_inputs": run_source_inputs,
            "build_lock": run_build_lock,
            "redirect": run_redirect,
            "descriptors": run_descriptors,
            "manifest": run_manifest,
            "extract": run_extract,
            "evidence": run_evidence,
            "evidence_check": run_evidence_check,
        }[mode](envelope())
    elif mode in ("record_ok", "record_bad", "archive_ok", "archive_bad"):
        {
            "record_ok": run_record_ok,
            "record_bad": run_record_bad,
            "archive_ok": run_archive_ok,
            "archive_bad": run_archive_bad,
        }[mode](sys.argv[3])
    elif mode == "tar_member":
        run_tar_member(sys.argv[3])
    else:
        raise SystemExit(2)


main()
"#;
