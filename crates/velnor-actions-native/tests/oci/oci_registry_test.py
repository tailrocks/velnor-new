"""Offline fake protocol regression coverage; never accesses a registry."""

import base64
import hashlib
import io
import importlib.util
import json
import tempfile
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace

def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).resolve().parents[2] / "src" / "oci" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


load("oci_digest")
load("oci_registry")
from oci_digest import GateError
from oci_registry import AUTH, HOST, Registry, _stream, publish_verified, registry_credentials


class Blob:
    def __init__(self, raw):
        self.raw = raw
        self.size = len(raw)
        self.digest = "sha256:" + hashlib.sha256(raw).hexdigest()

    def open(self):
        return io.BytesIO(self.raw)

    def metadata(self):
        return self.raw


class Fake:
    def __init__(self):
        self.calls, self.blobs, self.manifests = [], {}, {}
        self.location = "upload?_state=a%2Bb"
        self.events = []

    def __call__(self, host, method, path, headers, body=None, blob=None):
        self.calls.append((host, method, path, headers))
        if host == AUTH:
            assert headers["Authorization"].startswith("Basic ")
            return 200, {}, b'{"token":"opaque-token"}'
        if path == "/v2/":
            return 401, {"www-authenticate": 'Bearer realm="https://auth.docker.io/token",service="registry.docker.io"'}, b""
        assert host == HOST and headers["Authorization"] == "Bearer opaque-token"
        digest = path.rsplit("/", 1)[-1]
        if method == "HEAD":
            item = self.blobs.get(digest)
            return (404, {}, b"") if item is None else (200, {"docker-content-digest": digest, "content-length": str(item.size)}, b"")
        if method == "POST":
            self.events.append("post")
            return 202, {"location": self.location, "range": "0-0"}, b""
        if method == "PUT" and blob is not None:
            assert "_state=a%2Bb&digest=sha256%3A" in path
            assert headers["Content-Type"] == "application/octet-stream"
            _stream(SimpleNamespace(send=lambda chunk: None), blob)
            self.blobs[blob.digest] = blob
            return 201, {"docker-content-digest": blob.digest, "location": "/v2/team/image/blobs/" + blob.digest}, b""
        if method == "GET":
            raw = self.manifests.get(digest)
            return (404, {}, b"") if raw is None else (200, {"docker-content-digest": digest}, raw)
        if method == "PUT":
            self.events.append("manifest")
            self.manifests[digest] = body
            return 201, {"docker-content-digest": digest, "location": path}, b""
        raise AssertionError((method, path))


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.config = Path(self.temp.name)
        auth = base64.b64encode(b"user:password").decode()
        (self.config / "config.json").write_text(json.dumps({"auths": {"https://index.docker.io/v1/": {"auth": auth}}}))
        root = Blob(b'{"mediaType":"application/vnd.oci.image.index.v1+json"}')
        child = Blob(b'{"mediaType":"application/vnd.oci.image.manifest.v1+json"}')
        data = Blob(b"layer-content")
        self.archive = SimpleNamespace(image="team/image", digest=root.digest, manifests=(root, child), blobs={b.digest: b for b in (root, child, data)})

    def test_exact_graph_callbacks_and_idempotency(self):
        fake = Fake()
        def admission():
            fake.events.append("admission")
        def source():
            fake.events.append("source")
        digest = publish_verified(self.archive, "team/image", self.config, admission, source, fake)
        self.assertEqual(digest, self.archive.digest)
        self.assertEqual(fake.events, ["admission", "post", "source", "manifest", "source", "manifest"])
        self.assertEqual(list(fake.manifests), [self.archive.manifests[1].digest, self.archive.digest])
        self.assertEqual(publish_verified(self.archive, "team/image", self.config, admission, source, fake), digest)
        self.assertEqual(fake.events, ["admission", "post", "source", "manifest", "source", "manifest"])

    def test_cross_origin_and_bad_upload_locations(self):
        for location in ("https://evil.test/upload", "https://user@registry-1.docker.io/v2/team/image/blobs/uploads/id", "https://registry-1.docker.io:444/v2/team/image/blobs/uploads/id", "upload?digest=bad", "upload#fragment", "../other/id"):
            fake = Fake()
            fake.location = location
            with self.subTest(location=location), self.assertRaises(GateError):
                publish_verified(self.archive, "team/image", self.config, lambda: None, lambda: None, fake)
            self.assertFalse(any(method == "PUT" for _, method, _, _ in fake.calls))

    def test_existing_manifest_mismatch_refused(self):
        fake = Fake()
        fake.manifests[self.archive.digest] = b"different bytes"
        with self.assertRaises(GateError):
            publish_verified(self.archive, "team/image", self.config, lambda: None, lambda: None, fake)
        self.assertFalse(any(method in {"POST", "PUT"} for _, method, _, _ in fake.calls))

    def test_callback_failure_precedes_mutation(self):
        fake = Fake()
        def refuse():
            raise GateError("stale_source")
        with self.assertRaises(GateError):
            publish_verified(self.archive, "team/image", self.config, refuse, lambda: None, fake)
        self.assertFalse(any(method in {"POST", "PUT"} for _, method, _, _ in fake.calls))

    def test_stream_bounds_and_hash(self):
        blob = Blob(b"x")
        blob.digest = "sha256:" + "0" * 64
        with self.assertRaises(GateError):
            _stream(SimpleNamespace(send=lambda chunk: None), blob)
        blob = Blob(b"xy")
        blob.size = 1
        with self.assertRaises(GateError):
            _stream(SimpleNamespace(send=lambda chunk: None), blob)
        blob = Blob(b"x")
        blob.size = 2
        with self.assertRaises(GateError):
            _stream(SimpleNamespace(send=lambda chunk: None), blob)

    def test_credential_helpers_and_other_hosts_refused(self):
        for document in ({"credsStore": "execute-me", "auths": {}}, {"auths": {"evil.test": {"auth": "dTpw"}}}, {"auths": {"docker.io": {"auth": "not-base64"}}}):
            (self.config / "config.json").write_text(json.dumps(document))
            with self.subTest(document=document), self.assertRaises(GateError):
                registry_credentials(self.config)

    def test_registry_host_rejected(self):
        with self.assertRaises(GateError):
            Registry("evil.test/team/image", self.config, Fake())

    def test_expired_token_refresh_precedes_mutation_callback(self):
        fake = Fake()
        registry = Registry("team/image", self.config, fake)
        registry.deadline = 0
        before = len(fake.calls)
        observed = []
        blob = next(b for b in self.archive.blobs.values() if b not in self.archive.manifests)
        registry.put_blob(blob, lambda: observed.append(fake.calls[-1][0]))
        self.assertEqual(observed, [HOST])
        self.assertEqual(fake.calls[before][0], AUTH)
        self.assertEqual(sum(host == AUTH for host, _, _, _ in fake.calls), 2)

    def test_verified_image_identity_bound_before_network(self):
        fake = Fake()
        with self.assertRaises(GateError):
            publish_verified(self.archive, "team/other", self.config, lambda: None, lambda: None, fake)
        self.assertFalse(fake.calls)
        self.archive.image = "docker.io/team/image"
        self.assertEqual(publish_verified(self.archive, self.archive.image, self.config, lambda: None, lambda: None, fake), self.archive.digest)


if __name__ == "__main__":
    unittest.main()
