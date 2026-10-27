// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) const DRIVER_B: &str = r#"class _FakeResponse:
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

    def __init__(self, tamper_get=False):
        self.store = {}
        self.tamper_get = tamper_get
        self.puts = 0
        self.token_auth = ""

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

            self.token_auth = headers.get("Authorization", "")
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
        data = stored
        if self.tamper_get and "/manifests/" not in path:
            data = b"X" + stored[1:]
        return _FakeStream(
            200,
            {"docker-content-digest": digest, "content-length": str(len(stored))},
            data,
        )


def run_registry_token(case):
    try:
        registry = _FakeRegistry()
        token = publish_module.fetch_registry_token(
            registry_module, registry, case["actor"], case["password"]
        )
        import base64 as b64

        actor = (
            b64.b64decode(registry.token_auth.split(" ", 1)[1])
            .split(b":", 1)[0]
            .decode("ascii")
        )
    except ERRORS as error:
        report("TOKEN-REJECTED", error)
        return
    report("TOKEN-OK", f"{token[:4]} {actor}")


def registry_for(case, manifest):
    registry = _FakeRegistry(tamper_get=case.get("tamper_get", False))
    manifest_digest = "sha256:" + __import__("hashlib").sha256(manifest).hexdigest()
    for digest, body_hex in case.get("presend", {}).items():
        kind = "manifests" if digest == manifest_digest else "blobs"
        registry.store[f"{kind}:{digest}"] = bytes.fromhex(body_hex)
    return registry


def run_publish(case):
    try:
        manifest = bytes.fromhex(case["manifest_hex"])
        registry = registry_for(case, manifest)
        with tempfile.TemporaryDirectory(prefix="gc1-t46-publish-") as temp:
            blob_paths = {}
            for digest, body_hex in case["blobs"].items():
                path = Path(temp) / digest.removeprefix("sha256:")
                path.write_bytes(bytes.fromhex(body_hex))
                blob_paths[digest] = path
            proof = publish_module.publish_image(
                registry_module,
                registry,
                case.get("token", "test-token-0123456789abcdef"),
                manifest,
                blob_paths,
            )
    except ERRORS as error:
        report("PUBLISH-REJECTED", error)
        return
    report("PUBLISH-OK", f"puts={registry.puts} blobs={len(proof['blobs'])}")


def run_evidence(case):
    try:
        manifest = bytes.fromhex(case["manifest_hex"])
        with tempfile.TemporaryDirectory(prefix="gc1-t46-evidence-") as temp:
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
    case = envelope()
    {
        "identity": run_identity,
        "identity_file": run_identity_file,
        "repository": run_repository,
        "candidate_identity": run_candidate_identity,
        "artifact": run_artifact,
        "ci": run_ci,
        "attestation": run_attestation,
        "source_inputs": run_source_inputs,
        "build_lock": run_build_lock,
        "registry_url": run_registry_url,
        "download_location": run_download_location,
        "upload_location": run_upload_location,
        "upload_target": run_upload_target,
        "challenge": run_challenge,
        "token_url": run_token_url,
        "token": run_token,
        "registry_token": run_registry_token,
        "publish": run_publish,
        "head": run_head,
        "redirect": run_redirect,
        "get": run_get,
        "descriptors": run_descriptors,
        "manifest": run_manifest,
        "extract": run_extract,
        "evidence": run_evidence,
        "evidence_check": run_evidence_check,
    }[mode](case)


main()
"#;
