"""Allowlisted read-only GitHub API calls for release reconciliation."""

import os
import re
import urllib.error
import urllib.request
from urllib.parse import quote


GITHUB_API = "https://api.github.com/"
MAX_RESPONSE = 4 * 1024 * 1024
SHA = r"[0-9a-f]{40}"
REPOSITORY = r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+"
SOURCE_RESPONSE_LIMITS = {
    "source-repository": 1024 * 1024,
    "source-commit": 1024 * 1024,
    "source-tree": 8 * 1024 * 1024,
    "source-blob": 24 * 1024 * 1024,
}


class ForgeRedirect(urllib.request.HTTPRedirectHandler):
    """Reject every redirect so the token stays on the fixed GitHub host."""

    def redirect_request(self, request, file_pointer, code, message, headers, new_url):
        if file_pointer is not None:
            file_pointer.close()
        raise ReconcileError("forge_read_redirect")


def _read_token():
    token = os.environ.get("GH_TOKEN", "")
    require(token and len(token) <= 16384 and "\n" not in token and "\r" not in token,
            "forge_read_token")
    require(all(32 < ord(character) < 127 for character in token), "forge_read_token")
    return token


def _read_repository(repository):
    require(isinstance(repository, str) and re.fullmatch(REPOSITORY, repository) and
            all(part not in (".", "..") for part in repository.split("/")),
            "forge_read_repository")
    return repository


def _read_tag_path(repository, tag):
    require(isinstance(tag, str) and re.fullmatch(r"[A-Za-z0-9._/-]+", tag),
            "forge_read_tag")
    return f"repos/{repository}/git/ref/tags/{quote(tag, safe='')}"


def _read_tag_object_path(repository, sha):
    require(isinstance(sha, str) and re.fullmatch(SHA, sha), "forge_read_tag_object_sha")
    return f"repos/{repository}/git/tags/{sha}"


def _read_release_path(repository, tag):
    require(isinstance(tag, str) and re.fullmatch(r"[A-Za-z0-9._/-]+", tag),
            "forge_read_tag")
    return f"repos/{repository}/releases/tags/{quote(tag, safe='')}"


def _read_endpoint_kind(repository, endpoint):
    if type(endpoint) is str and endpoint == f"repos/{repository}":
        return "source-repository"
    prefix = f"repos/{repository}/"
    require(isinstance(endpoint, str) and endpoint.startswith(prefix), "forge_read_endpoint")
    tail = endpoint[len(prefix):]
    if re.fullmatch(r"git/ref/tags/[A-Za-z0-9._%2F-]+", tail):
        return "tag-ref"
    if re.fullmatch(r"git/tags/" + SHA, tail):
        return "tag-object"
    if re.fullmatch(r"releases/tags/[A-Za-z0-9._%2F-]+", tail):
        return "release"
    if re.fullmatch(r"git/commits/" + SHA, tail):
        return "source-commit"
    if re.fullmatch(r"git/trees/" + SHA + r"\?recursive=1", tail):
        return "source-tree"
    if re.fullmatch(r"git/blobs/" + SHA, tail):
        return "source-blob"
    raise ReconcileError("forge_read_endpoint")


def _read_response_body(response, kind):
    limit = SOURCE_RESPONSE_LIMITS.get(kind, MAX_RESPONSE)
    body = response.read(limit + 1)
    require(len(body) <= limit, "forge_read_response_size")
    if not body:
        return {}
    try:
        return decode_json(body)
    except (TypeError, ValueError) as error:
        raise ReconcileError("forge_read_response_json") from error


def forge_read_request(repository, endpoint):
    """Perform one exact GET against the fixed GitHub REST API host."""
    repository = _read_repository(repository)
    kind = _read_endpoint_kind(repository, endpoint)
    request = urllib.request.Request(
        GITHUB_API + endpoint,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": "Bearer " + _read_token(),
            "User-Agent": "velnor-fixed-release-publisher/1",
            "X-GitHub-Api-Version": "2022-11-28",
        },
        method="GET",
    )
    opener = urllib.request.build_opener(ForgeRedirect())
    try:
        with opener.open(request, timeout=40) as response:
            require(response.status == 200, "forge_read_status")
            return _read_response_body(response, kind)
    except urllib.error.HTTPError as error:
        code = error.code
        error.close()
        if code == 404:
            return None
        raise ReconcileError(f"forge_read_http_{code}") from None
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        raise ReconcileError("forge_read_transport") from error


def read_tag_ref(repository, tag):
    return forge_read_request(repository, _read_tag_path(repository, tag))


def read_tag_object(repository, sha):
    return forge_read_request(repository, _read_tag_object_path(repository, sha))


def read_release(repository, tag):
    return forge_read_request(repository, _read_release_path(repository, tag))


def _read_source_object(repository, sha, resource):
    require(isinstance(sha, str) and re.fullmatch(SHA, sha), "forge_read_source_sha")
    require(resource in ("commits", "trees", "blobs"), "forge_read_source_resource")
    suffix = "?recursive=1" if resource == "trees" else ""
    value = forge_read_request(repository, f"repos/{repository}/git/{resource}/{sha}{suffix}")
    if value is not None:
        require(isinstance(value, dict) and value.get("sha") == sha, "forge_read_source_identity")
    return value


def read_source_commit(repository, sha):
    return _read_source_object(repository, sha, "commits")


def read_source_repository(repository):
    repository = _read_repository(repository)
    value = forge_read_request(repository, f"repos/{repository}")
    require(type(value) is dict and type(value.get("id")) is int and
            0 < value["id"] <= 2**64 - 1 and
            type(value.get("full_name")) is str and value["full_name"] == repository,
            "forge_read_source_repository_identity")
    return value


def read_source_tree(repository, sha):
    return _read_source_object(repository, sha, "trees")


def read_source_blob(repository, sha):
    return _read_source_object(repository, sha, "blobs")
