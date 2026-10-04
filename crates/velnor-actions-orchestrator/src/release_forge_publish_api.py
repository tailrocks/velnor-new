"""Allowlisted GitHub POST calls for immutable release publication."""

import json
import os
import re
import urllib.error
import urllib.request


GITHUB_API = "https://api.github.com/"
MAX_RESPONSE = 4 * 1024 * 1024
SHA = r"[0-9a-f]{40}"
REPOSITORY = r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+"


class ForgeWriteUncertain(ReconcileError):
    """A write may have reached GitHub but lacks a trustworthy response."""


class ForgeWriteCollision(ReconcileError):
    """GitHub rejected a write because remote state may already exist."""


class ForgeWriteRedirect(urllib.request.HTTPRedirectHandler):
    """Reject every redirect so the token stays on the fixed GitHub host."""

    def redirect_request(self, request, file_pointer, code, message, headers, new_url):
        raise ReconcileError("forge_publish_redirect")


def _write_token():
    token = os.environ.get("GH_TOKEN", "")
    require(token and len(token) <= 16384 and "\n" not in token and "\r" not in token,
            "forge_publish_token")
    require(all(32 < ord(character) < 127 for character in token), "forge_publish_token")
    return token


def _write_repository(repository):
    require(isinstance(repository, str) and re.fullmatch(REPOSITORY, repository),
            "forge_publish_repository")
    return repository


def _write_endpoint_kind(repository, endpoint):
    prefix = f"repos/{repository}/"
    require(isinstance(endpoint, str) and endpoint.startswith(prefix), "forge_publish_endpoint")
    tail = endpoint[len(prefix):]
    if tail == "git/tags":
        return "tag-create"
    if tail == "git/refs":
        return "ref-create"
    if tail == "releases":
        return "release-create"
    raise ReconcileError("forge_publish_endpoint")


def _write_payload_bytes(payload):
    require(isinstance(payload, dict), "forge_publish_payload")
    try:
        encoded = json.dumps(payload, ensure_ascii=False, separators=(",", ":"),
                             allow_nan=False).encode("utf-8")
    except (TypeError, ValueError) as error:
        raise ReconcileError("forge_publish_payload") from error
    require(len(encoded) <= 1024 * 1024, "forge_publish_payload_size")
    return encoded


def _write_response_body(response):
    body = response.read(MAX_RESPONSE + 1)
    require(len(body) <= MAX_RESPONSE, "forge_publish_response_size")
    if not body:
        return {}
    try:
        return decode_json(body)
    except (TypeError, ValueError) as error:
        raise ReconcileError("forge_publish_response_json") from error


def forge_request(repository, method, endpoint, payload=None):
    """Perform one exact POST against the fixed GitHub REST API host."""
    repository = _write_repository(repository)
    require(method == "POST", "forge_publish_method")
    kind = _write_endpoint_kind(repository, endpoint)
    _validate_write_payload(kind, payload)
    request = urllib.request.Request(
        GITHUB_API + endpoint,
        data=_write_payload_bytes(payload),
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": "Bearer " + _write_token(),
            "Content-Type": "application/json",
            "User-Agent": "velnor-fixed-release-publisher/1",
            "X-GitHub-Api-Version": "2022-11-28",
        },
        method="POST",
    )
    opener = urllib.request.build_opener(ForgeWriteRedirect())
    try:
        with opener.open(request, timeout=40) as response:
            if getattr(response, "status", None) != 201:
                raise ForgeWriteUncertain("forge_publish_status")
            try:
                return _write_response_body(response)
            except ReconcileError as error:
                raise ForgeWriteUncertain("forge_publish_response") from error
    except urllib.error.HTTPError as error:
        if error.code in (409, 422):
            raise ForgeWriteCollision("forge_publish_collision") from None
        if error.code in (408, 425, 429) or error.code >= 500:
            raise ForgeWriteUncertain(f"forge_publish_http_{error.code}") from None
        raise ReconcileError(f"forge_publish_http_{error.code}") from None
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        raise ForgeWriteUncertain("forge_publish_transport") from error


def _validate_write_payload(kind, payload):
    if kind == "tag-create":
        fields = {"tag", "message", "object", "type"}
        require(isinstance(payload, dict) and set(payload) == fields and
                payload["type"] == "commit" and isinstance(payload["object"], str) and
                re.fullmatch(SHA, payload["object"]),
                "forge_publish_tag_payload")
        require(isinstance(payload["tag"], str) and
                re.fullmatch(r"[A-Za-z0-9._/-]+", payload["tag"]) and
                isinstance(payload["message"], str) and 0 < len(payload["message"])
                <= 1024, "forge_publish_tag_payload")
    elif kind == "ref-create":
        require(isinstance(payload, dict) and set(payload) == {"ref", "sha"} and
                isinstance(payload["ref"], str) and
                re.fullmatch(r"refs/tags/[A-Za-z0-9._/-]+", payload["ref"]) and
                isinstance(payload["sha"], str) and re.fullmatch(SHA, payload["sha"]),
                "forge_publish_ref_payload")
    else:
        fields = {"tag_name", "body", "name", "draft", "prerelease"}
        require(isinstance(payload, dict) and set(payload) == fields and
                isinstance(payload["tag_name"], str) and
                re.fullmatch(r"[A-Za-z0-9._/-]+", payload["tag_name"]) and
                isinstance(payload["body"], str) and 0 <= len(payload["body"]) <= 65536 and
                isinstance(payload["name"], str) and 0 < len(payload["name"]) <= 256 and
                payload["draft"] is False and type(payload["prerelease"]) is bool,
                "forge_publish_release_payload")


def create_tag(repository, payload):
    return forge_request(repository, "POST", f"repos/{repository}/git/tags", payload)


def create_tag_ref(repository, payload):
    return forge_request(repository, "POST", f"repos/{repository}/git/refs", payload)


def create_release(repository, payload):
    return forge_request(repository, "POST", f"repos/{repository}/releases", payload)
