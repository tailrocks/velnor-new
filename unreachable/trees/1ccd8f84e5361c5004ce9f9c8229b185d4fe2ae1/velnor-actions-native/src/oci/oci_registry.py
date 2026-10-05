"""Bounded Docker Hub publication of an already verified OCI graph."""

import base64
import hashlib
import http.client
import json
import os
import re
import ssl
import stat
import time
import urllib.parse
from pathlib import Path

from oci_digest import GateError, need

HOST = "registry-1.docker.io"
AUTH = "auth.docker.io"
REALM = "https://auth.docker.io/token"
SERVICE = "registry.docker.io"
BOUND = 16 * 1024 * 1024
CHUNK = 1024 * 1024
DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")
HOST_KEYS = {"docker.io", "registry-1.docker.io", "https://index.docker.io/v1/"}


def repository_name(image):
    need(isinstance(image, str), "image_repository")
    value = image.removeprefix("docker.io/")
    need(len(value) < 256, "image_repository")
    pattern = r"[a-z0-9]+(?:[._-][a-z0-9]+)*(?:/[a-z0-9]+(?:[._-][a-z0-9]+)*)+"
    need(re.fullmatch(pattern, value) is not None, "image_repository")
    need("." not in value.split("/")[0], "image_registry")
    return value


def _json(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            need(key not in result, "registry_json_duplicate")
            result[key] = value
        return result
    try:
        value = json.loads(raw, object_pairs_hook=unique)
    except (ValueError, UnicodeError) as error:
        raise GateError("registry_json") from error
    need(isinstance(value, dict), "registry_json")
    return value


def registry_credentials(docker_config):
    root = Path(docker_config)
    descriptor = None
    try:
        info = root.lstat()
        need(stat.S_ISDIR(info.st_mode), "docker_config")
        descriptor = os.open(root / "config.json", os.O_RDONLY | os.O_NOFOLLOW)
        info = os.fstat(descriptor)
        need(stat.S_ISREG(info.st_mode) and info.st_size <= BOUND, "docker_config")
        with os.fdopen(descriptor, "rb") as source:
            descriptor = None
            raw = source.read(BOUND + 1)
        need(len(raw) <= BOUND, "docker_config")
        value = _json(raw)
    except OSError as error:
        raise GateError("docker_config") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)
    need("credsStore" not in value and "credHelpers" not in value, "docker_credential_helper")
    auths = value.get("auths")
    need(isinstance(auths, dict) and auths and set(auths) <= HOST_KEYS, "docker_auths")
    entries = []
    for entry in auths.values():
        need(isinstance(entry, dict) and set(entry) == {"auth"}, "docker_credentials")
        try:
            raw = base64.b64decode(entry["auth"], validate=True).decode("utf-8")
        except (ValueError, UnicodeError, TypeError) as error:
            raise GateError("docker_credentials") from error
        user, separator, password = raw.partition(":")
        need(separator and user and password and not any(ord(c) < 32 for c in raw), "docker_credentials")
        entries.append(raw)
    need(len(set(entries)) == 1, "docker_credentials_conflict")
    return entries[0]


def _stream(connection, blob):
    total, hasher = 0, hashlib.sha256()
    with blob.open() as source:
        while total < blob.size:
            chunk = source.read(min(CHUNK, blob.size - total))
            need(chunk and len(chunk) <= blob.size - total, "registry_blob_size")
            total += len(chunk)
            hasher.update(chunk)
            connection.send(chunk)
        need(not source.read(1), "registry_blob_size")
    need(total == blob.size and "sha256:" + hasher.hexdigest() == blob.digest, "registry_blob_digest")


def _http(host, method, path, headers, body=None, blob=None):
    need(host in {HOST, AUTH}, "registry_host")
    need(path.startswith("/") and not any(ord(c) < 32 for c in path), "registry_path")
    connection = http.client.HTTPSConnection(host, timeout=90, context=ssl.create_default_context())
    try:
        connection.putrequest(method, path, skip_accept_encoding=True)
        for key, value in headers.items():
            connection.putheader(key, value)
        if blob is not None or body is not None:
            connection.putheader("Content-Length", str(blob.size if blob is not None else len(body)))
        connection.endheaders()
        if blob is not None:
            _stream(connection, blob)
        elif body is not None:
            connection.send(body)
        response = connection.getresponse()
        raw = response.read(BOUND + 1)
        need(len(raw) <= BOUND, "registry_response_bound")
        pairs = response.getheaders()
        lowered = [key.lower() for key, _ in pairs]
        need(len(lowered) == len(set(lowered)), "registry_duplicate_header")
        need(not 300 <= response.status < 400, "registry_redirect")
        return response.status, dict((key.lower(), value) for key, value in pairs), raw
    except (OSError, http.client.HTTPException) as error:
        raise GateError("registry_request") from error
    finally:
        connection.close()


def _challenge(value, repository):
    pattern = r'Bearer realm="([^"]+)",service="([^"]+)"(?:,scope="([^"]+)")?'
    match = re.fullmatch(pattern, value)
    need(match is not None, "registry_challenge")
    realm, service, scope = match.groups()
    need(realm == REALM and service == SERVICE, "registry_challenge")
    need(scope is None or scope == "repository:" + repository + ":pull,push", "registry_scope")


class Registry:
    """Fixed origins, no redirects, digest references only."""

    def __init__(self, image, docker_config, http=None):
        self.repository = repository_name(image)
        self.prefix = "/v2/" + self.repository
        self.http = http or _http
        credentials = registry_credentials(docker_config)
        status, headers, _ = self.http(HOST, "GET", "/v2/", {})
        need(status == 401, "registry_auth")
        _challenge(headers.get("www-authenticate", ""), self.repository)
        self.basic = base64.b64encode(credentials.encode()).decode("ascii")
        self._refresh()

    def _refresh(self):
        query = urllib.parse.urlencode({"service": SERVICE, "scope": "repository:" + self.repository + ":pull,push"})
        started = time.monotonic()
        status, _, raw = self.http(AUTH, "GET", "/token?" + query, {"Authorization": "Basic " + self.basic})
        need(status == 200, "registry_token")
        value = _json(raw)
        token = value.get("token", value.get("access_token"))
        need("token" not in value or "access_token" not in value or value["token"] == value["access_token"], "registry_token_conflict")
        need(isinstance(token, str) and token and not any(ord(c) <= 32 for c in token), "registry_token")
        expiry = value.get("expires_in", 60)
        need(type(expiry) is int and 0 < expiry <= 86400, "registry_token_expiry")
        self.deadline = started + expiry - min(30, expiry / 2)
        need(time.monotonic() < self.deadline, "registry_token_expired")
        self.headers = {"Authorization": "Bearer " + token}

    def request(self, method, path, body=None, blob=None, media=None, before_send=None):
        need(path.startswith(self.prefix + "/"), "registry_path")
        if time.monotonic() >= self.deadline:
            self._refresh()
        headers = dict(self.headers)
        if media:
            headers["Content-Type"] = media
            headers["Accept"] = media
        if before_send is not None:
            before_send()
        result = self.http(HOST, method, path, headers, body=body, blob=blob)
        need(not 300 <= result[0] < 400, "registry_redirect")
        return result

    def location(self, value, base, upload=False):
        need(isinstance(value, str) and value and not any(ord(c) <= 32 for c in value), "registry_location")
        parsed = urllib.parse.urlsplit(urllib.parse.urljoin("https://" + HOST + base, value))
        need(parsed.scheme == "https" and parsed.netloc in {HOST, HOST + ":443"}, "registry_location")
        need(not parsed.fragment and not parsed.username and not parsed.password, "registry_location")
        prefix = self.prefix + ("/blobs/uploads/" if upload else "/")
        suffix = parsed.path.removeprefix(prefix)
        need(parsed.path.startswith(prefix), "registry_location")
        need(not upload or (suffix and "/" not in suffix), "registry_location")
        need(not upload or re.fullmatch(r"[A-Za-z0-9._-]+", suffix) is not None, "registry_location")
        need(not any(key == "digest" for key, _ in urllib.parse.parse_qsl(parsed.query)), "registry_location_digest")
        return parsed.path + ("?" + parsed.query if parsed.query else "")

    def has_blob(self, blob):
        status, headers, _ = self.request("HEAD", self.prefix + "/blobs/" + blob.digest)
        if status == 404:
            return False
        need(status == 200 and headers.get("docker-content-digest") == blob.digest, "blob_head")
        need(headers.get("content-length") == str(blob.size), "blob_head_size")
        return True

    def put_blob(self, blob, mutate):
        if self.has_blob(blob):
            return
        base = self.prefix + "/blobs/uploads/"
        status, headers, _ = self.request("POST", base, body=b"", before_send=mutate)
        need(status == 202 and headers.get("range") in {None, "0-0"}, "blob_upload_start")
        target = self.location(headers.get("location"), base, upload=True)
        separator = "&" if "?" in target else "?"
        target += separator + "digest=" + urllib.parse.quote(blob.digest, safe="")
        status, headers, _ = self.request("PUT", target, blob=blob, media="application/octet-stream")
        need(status == 201 and headers.get("docker-content-digest") == blob.digest, "blob_upload")
        location = self.location(headers.get("location"), target)
        need(urllib.parse.urlsplit(location).path == self.prefix + "/blobs/" + blob.digest, "blob_upload_location")
        need(self.has_blob(blob), "blob_verify")

    def has_manifest(self, manifest):
        digest, raw = manifest.digest, manifest.metadata()
        media = _json(raw).get("mediaType")
        need(media in {"application/vnd.oci.image.index.v1+json", "application/vnd.oci.image.manifest.v1+json"}, "manifest_media")
        need(DIGEST.fullmatch(digest) and hashlib.sha256(raw).hexdigest() == digest[7:], "manifest_digest")
        path = self.prefix + "/manifests/" + digest
        status, headers, existing = self.request("GET", path, media=media)
        if status == 200:
            need(headers.get("docker-content-digest") == digest and existing == raw, "manifest_existing")
            return True
        need(status == 404, "manifest_get")
        return False

    def put_manifest(self, manifest, mutate, before_manifest):
        if self.has_manifest(manifest):
            return
        digest, raw = manifest.digest, manifest.metadata()
        media = _json(raw)["mediaType"]
        path = self.prefix + "/manifests/" + digest
        def admission():
            mutate()
            before_manifest()
        status, headers, _ = self.request("PUT", path, body=raw, media=media, before_send=admission)
        need(status == 201 and headers.get("docker-content-digest") == digest, "manifest_put")
        location = self.location(headers.get("location"), path)
        need(urllib.parse.urlsplit(location).path == path, "manifest_location")


def publish_verified(archive, image, docker_config, before_mutation, before_manifest, http=None):
    need(image == archive.image, "registry_archive_image")
    registry = Registry(image, docker_config, http)
    started = False
    def mutate():
        nonlocal started
        if not started:
            before_mutation()
            started = True
    need(archive.manifests and archive.manifests[0].digest == archive.digest, "registry_root")
    registry.has_manifest(archive.manifests[0])
    manifests = {manifest.digest for manifest in archive.manifests}
    for digest, blob in archive.blobs.items():
        if digest not in manifests:
            registry.put_blob(blob, mutate)
    for manifest in (*archive.manifests[1:], archive.manifests[0]):
        registry.put_manifest(manifest, mutate, before_manifest)
    return archive.digest
