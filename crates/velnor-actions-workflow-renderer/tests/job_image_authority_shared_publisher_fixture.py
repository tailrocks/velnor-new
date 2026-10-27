# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import hashlib
import io
import json
import tarfile
from urllib.parse import parse_qs, urlsplit


class Response:
    def __init__(self, status, headers, body=b""):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self.body = body


class Stream:
    def __init__(self, status, headers, body):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self.body = body
        self.offset = 0

    def read(self, size=-1):
        if size < 0:
            size = len(self.body) - self.offset
        chunk = self.body[self.offset:self.offset + size]
        self.offset += len(chunk)
        return chunk

    def close(self):
        pass


class RegistryIO:
    def __init__(self, package, tamper_readback=None):
        self.package = package
        self.tamper_readback = tamper_readback
        self.blobs = {}
        self.manifests = {}
        self.tags = {}
        self.token_scope = None
        self.api_paths = []
        self.manifest_puts = []

    def _resource(self, path):
        prefix = f"/v2/{self.package}/"
        if not path.startswith(prefix):
            raise AssertionError(f"registry request escaped package scope: {path}")
        resource = path.removeprefix(prefix)
        self.api_paths.append(resource)
        return resource

    def _manifest(self, reference):
        digest = self.tags.get(reference, reference)
        return digest, self.manifests.get(digest)

    def request(self, method, url, headers, body=None, length=0):
        parts = urlsplit(url)
        if method == "GET" and parts.path == "/v2/":
            challenge = (
                'Bearer realm="https://ghcr.io/token",service="ghcr.io",'
                f' scope="repository:{self.package}:pull,push"'
            )
            return Response(401, {"www-authenticate": challenge})
        if method == "GET" and parts.path == "/token":
            self.token_scope = parse_qs(parts.query)["scope"][0]
            return Response(200, {}, b'{"token":"fixture-token-0123456789abcdef"}')
        resource = self._resource(parts.path)
        if method == "HEAD":
            if resource.startswith("blobs/"):
                digest = resource.removeprefix("blobs/")
                data = self.blobs.get(digest)
            elif resource.startswith("manifests/"):
                digest, data = self._manifest(resource.removeprefix("manifests/"))
            else:
                raise AssertionError(f"unexpected registry HEAD: {resource}")
            if data is None:
                return Response(404, {})
            return Response(
                200,
                {"docker-content-digest": digest, "content-length": str(len(data))},
            )
        if method == "POST" and resource == "blobs/uploads/":
            return Response(
                202,
                {"location": f"/v2/{self.package}/blobs/uploads/fixture-session"},
            )
        if method == "PUT" and resource.startswith("blobs/uploads/"):
            digest = parse_qs(parts.query)["digest"][0]
            chunks = []
            with body.open("rb") as source:
                while chunk := source.read(65536):
                    chunks.append(chunk)
            data = b"".join(chunks)
            if len(data) != length:
                raise AssertionError("blob upload length changed")
            self.blobs[digest] = data
            return Response(201, {})
        if method == "PUT" and resource.startswith("manifests/"):
            reference = resource.removeprefix("manifests/")
            if not isinstance(body, bytes) or len(body) != length:
                raise AssertionError("manifest upload must preserve exact bytes")
            digest = "sha256:" + hashlib.sha256(body).hexdigest()
            self.manifests[digest] = body
            self.manifest_puts.append(reference)
            if reference.startswith("sha-"):
                self.tags[reference] = digest
            return Response(201, {"docker-content-digest": digest})
        raise AssertionError(f"unexpected registry request: {method} {url}")

    def stream(self, method, url, headers):
        parts = urlsplit(url)
        resource = self._resource(parts.path)
        if method != "GET":
            raise AssertionError(f"unexpected registry stream: {method} {url}")
        if resource.startswith("blobs/"):
            digest = resource.removeprefix("blobs/")
            data = self.blobs[digest]
            kind = "blob"
        elif resource.startswith("manifests/"):
            digest, data = self._manifest(resource.removeprefix("manifests/"))
            kind = "manifest"
        else:
            raise AssertionError(f"unexpected registry stream: {resource}")
        source_tag = (
            kind == "manifest"
            and resource.removeprefix("manifests/").startswith("sha-")
        )
        if self.tamper_readback == kind or (
            self.tamper_readback == "source-tag" and source_tag
        ):
            data = bytes([data[0] ^ 1]) + data[1:]
        return Stream(
            200,
            {"docker-content-digest": digest, "content-length": str(len(data))},
            data,
        )

    def seed_tag(self, tag, manifest):
        digest = "sha256:" + hashlib.sha256(manifest).hexdigest()
        self.manifests[digest] = manifest
        self.tags[tag] = digest


def add_member(archive, path, data):
    member = tarfile.TarInfo(path)
    member.size = len(data)
    member.mode = 0o644
    member.mtime = 0
    archive.addfile(member, io.BytesIO(data))


def make_oci_archive(path):
    layer_file = io.BytesIO()
    with tarfile.open(fileobj=layer_file, mode="w") as layer:
        add_member(layer, "fixture.txt", b"oci-fixture")
    layer_bytes = layer_file.getvalue()
    config = json.dumps(
        {
            "architecture": "amd64",
            "os": "linux",
            "rootfs": {
                "type": "layers",
                "diff_ids": ["sha256:" + hashlib.sha256(layer_bytes).hexdigest()],
            },
        },
        separators=(",", ":"),
    ).encode()
    config_digest = "sha256:" + hashlib.sha256(config).hexdigest()
    layer_digest = "sha256:" + hashlib.sha256(layer_bytes).hexdigest()
    manifest = json.dumps(
        {
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": config_digest,
                "size": len(config),
            },
            "layers": [
                {
                    "mediaType": "application/vnd.oci.image.layer.v1.tar",
                    "digest": layer_digest,
                    "size": len(layer_bytes),
                }
            ],
        },
        separators=(",", ":"),
    ).encode()
    manifest_digest = "sha256:" + hashlib.sha256(manifest).hexdigest()
    index = json.dumps(
        {
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [
                {
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "digest": manifest_digest,
                    "size": len(manifest),
                    "platform": {"architecture": "amd64", "os": "linux"},
                }
            ],
        },
        separators=(",", ":"),
    ).encode()
    with tarfile.open(path, mode="w") as archive:
        add_member(archive, "oci-layout", b'{"imageLayoutVersion":"1.0.0"}')
        add_member(archive, "index.json", index)
        for digest, data in (
            (manifest_digest, manifest),
            (config_digest, config),
            (layer_digest, layer_bytes),
        ):
            add_member(archive, "blobs/sha256/" + digest.removeprefix("sha256:"), data)
