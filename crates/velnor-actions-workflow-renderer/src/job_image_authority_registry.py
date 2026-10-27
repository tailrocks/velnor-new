# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""Fixed OCI registry origin, redirect, token, and response checks.

Data-only port of the registry logic in
``images/job/ubuntu-26.04/candidate/publish.py``. The rendered authority
workflow performs the bounded HTTPS calls; this module only decides which
URLs may be used, which redirects may be followed without credentials, and
whether a received response proves the requested bytes. It never opens a
network path and never sees registry credentials.
"""
import hashlib
import json
import re
from urllib.parse import parse_qsl, unquote, urlencode, urljoin, urlsplit, urlunsplit

REGISTRY = "ghcr.io"
DOWNLOAD_HOST = "pkg-containers.githubusercontent.com"
PACKAGE = "tailrocks/velnor-job"


class AuthorityError(ValueError):
    pass


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise AuthorityError(f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def _reject_constant(value):
    raise AuthorityError(f"JSON constant has an incorrect value: {value}.")


def _token_json(data, label):
    try:
        return json.loads(data, object_pairs_hook=_unique_object, parse_constant=_reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AuthorityError(f"{label} has an incorrect format.") from error


def registry_url(path):
    return f"https://{REGISTRY}/v2/{PACKAGE}/{path}"


def blob_download_location(location, digest):
    if any(ord(char) < 32 or ord(char) == 127 for char in location) or "\\" in location:
        raise AuthorityError("Registry download URL has an incorrect character.")
    try:
        parts = urlsplit(location)
    except ValueError as error:
        raise AuthorityError("Registry download URL has an incorrect format.") from error
    if (
        parts.scheme != "https"
        or parts.hostname != DOWNLOAD_HOST
        or parts.port not in (None, 443)
        or parts.username is not None
        or parts.password is not None
        or parts.fragment
        or not parts.path.endswith(f"/blobs/{digest}")
    ):
        raise AuthorityError("Registry download URL is outside the permitted HTTPS location.")
    decoded = unquote(parts.path + parts.query)
    if any(ord(char) < 32 or ord(char) == 127 for char in decoded) or "\\" in decoded:
        raise AuthorityError("Registry download URL has an incorrect character.")
    return urlunsplit(("", "", parts.path, parts.query, ""))


def validate_upload_location(base, location):
    if not location or len(location) > 4096 or "\\" in location or any(ord(char) < 32 or ord(char) == 127 for char in location):
        raise AuthorityError("Registry upload location is missing or too long.")
    try:
        location_parts = urlsplit(location)
        raw_path = unquote(location_parts.path)
        resolved = urljoin(base, location)
        parsed = urlsplit(resolved)
        port = parsed.port
    except ValueError as error:
        raise AuthorityError("Registry upload location is outside the fixed HTTPS upload path.") from error
    if any(part in {".", ".."} for part in raw_path.split("/")):
        raise AuthorityError("Registry upload location is outside the fixed HTTPS upload path.")
    decoded_path = unquote(parsed.path)
    expected_prefixes = (
        f"/v2/{PACKAGE}/blobs/upload/",
        f"/v2/{PACKAGE}/blobs/uploads/",
    )
    query_values = parse_qsl(parsed.query, keep_blank_values=True)
    if (
        parsed.scheme != "https"
        or parsed.hostname != REGISTRY
        or port not in (None, 443)
        or parsed.username is not None
        or parsed.password is not None
        or parsed.fragment
        or not decoded_path.startswith(expected_prefixes)
        or "\\" in decoded_path
        or any(ord(char) < 32 for char in decoded_path)
        or any(ord(char) == 127 for char in decoded_path)
        or any(ord(char) < 32 or ord(char) == 127 for key, value in query_values for char in key + value)
        or any(part in {".", ".."} for part in decoded_path.split("/"))
    ):
        raise AuthorityError("Registry upload location is outside the fixed HTTPS upload path.")
    if any(not key for key, _ in query_values):
        raise AuthorityError("Registry upload query has an incorrect format.")
    return resolved


def upload_target(location, digest):
    try:
        parts = urlsplit(location)
    except ValueError as error:
        raise AuthorityError("Registry upload location is outside the fixed HTTPS upload path.") from error
    query = parse_qsl(parts.query, keep_blank_values=True)
    if any(key == "digest" for key, _ in query):
        raise AuthorityError("Registry upload location contains a digest parameter.")
    if not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
        raise AuthorityError("OCI blob descriptor is incorrect.")
    digest_query = urlencode({"digest": digest})
    merged = f"{parts.query}&{digest_query}" if parts.query else digest_query
    return urlunsplit((parts.scheme, parts.netloc, parts.path, merged, ""))


def parse_auth_challenge(value):
    if not isinstance(value, str):
        raise AuthorityError("Registry authentication challenge has an incorrect format.")
    match = re.fullmatch(r"Bearer\s+(.+)", value)
    if match is None:
        raise AuthorityError("Registry authentication challenge has an incorrect format.")
    params = {}
    remaining = match.group(1)
    while remaining:
        part = re.match(r'\s*([A-Za-z][A-Za-z0-9_-]*)="([^"\\]*)"\s*(?:,|$)', remaining)
        if part is None or part.group(1) in params:
            raise AuthorityError("Registry authentication challenge has an incorrect format.")
        params[part.group(1)] = part.group(2)
        remaining = remaining[part.end():]
    if (
        set(params) - {"realm", "service", "scope"}
        or params.get("realm") != f"https://{REGISTRY}/token"
        or params.get("service") != REGISTRY
    ):
        raise AuthorityError("Registry authentication challenge has an incorrect format.")
    return params


def token_request_url(params):
    if not isinstance(params, dict) or set(params) - {"realm", "service", "scope"}:
        raise AuthorityError("Registry authentication challenge has an incorrect format.")
    query = urlencode({"service": "ghcr.io", "scope": f"repository:{PACKAGE}:pull,push"})
    return f"https://{REGISTRY}/token?{query}"


def parse_token_payload(data):
    value = _token_json(data, "Registry token response")
    token = value.get("token", value.get("access_token")) if isinstance(value, dict) else None
    if not isinstance(token, str) or not re.fullmatch(r"[A-Za-z0-9._~=-]{16,8192}", token):
        raise AuthorityError("Registry token response has no token in the required format.")
    return token


def check_head_response(status, headers, digest, size):
    if status == 404:
        return False
    if status != 200 or headers.get("docker-content-digest") != digest:
        raise AuthorityError("Registry digest HEAD verification failed.")
    if headers.get("content-length") != str(size):
        raise AuthorityError("Registry size HEAD verification failed.")
    return True


def redirect_target(path, digest, status, location):
    if status not in (302, 307):
        return None
    if path != f"/v2/{PACKAGE}/blobs/{digest}":
        raise AuthorityError("Registry manifest redirects are not permitted.")
    return blob_download_location(location, digest)


def check_get_response(status, headers, body, digest, size, expected):
    if status != 200:
        raise AuthorityError(f"Registry GET status is incorrect: {status}.")
    if headers.get("content-length") != str(size):
        raise AuthorityError("Registry GET size is different from the specified size.")
    if headers.get("docker-content-digest") not in (None, digest):
        raise AuthorityError("Registry GET digest header is different from the requested digest.")
    if len(body) != size:
        raise AuthorityError("Registry GET size is different from the specified size.")
    if hashlib.sha256(body).hexdigest() != digest.removeprefix("sha256:"):
        raise AuthorityError("Registry response bytes have an incorrect digest or size.")
    if body != expected:
        raise AuthorityError("Registry retrieved bytes changed during publication.")
    return len(body)
