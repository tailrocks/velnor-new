"""Fail-closed validation for release and immutable commit action pins."""

import json
import re

FULL_SHA = re.compile(r"[0-9a-f]{40}\Z")
SEMVER_LABEL = re.compile(r"v[0-9]+\.[0-9]+\.[0-9]+\Z")
COMMIT_API = re.compile(
    r"https://api\.github\.com/repos/([A-Za-z0-9_.-]+)/"
    r"([A-Za-z0-9_.-]+)/commits/([0-9a-f]{40})\Z"
)
TAG_COMMIT_API = re.compile(
    r"https://api\.github\.com/repos/([A-Za-z0-9_.-]+)/"
    r"([A-Za-z0-9_.-]+)/commits/(v[0-9]+\.[0-9]+\.[0-9]+)\Z"
)
LATEST_RELEASE_PATHS = ("releases/latest", "releases")


def _github_action_repository(action_key):
    """Return a validated owner/repository pair, stripping action subpaths."""
    parts = action_key.split("/") if isinstance(action_key, str) else []
    if len(parts) < 2 or not all(parts[:2]):
        raise ValueError("action key must include owner and repository")
    if any(re.fullmatch(r"[A-Za-z0-9_.-]+", part) is None for part in parts):
        raise ValueError("action key contains an invalid path segment")
    return parts[0], parts[1]


def github_commit_source(action_key, sha):
    """Return the repository commit endpoint, stripping any action subpath."""
    if not isinstance(sha, str) or FULL_SHA.fullmatch(sha) is None:
        raise ValueError("commit API source requires a full lowercase SHA")
    owner, repository = _github_action_repository(action_key)
    return (
        f"https://api.github.com/repos/{owner}/{repository}/commits/{sha}"
    )


def github_tag_commit_source(action_key, version):
    """Return the exact repository endpoint resolving a release tag to SHA."""
    if not isinstance(version, str) or SEMVER_LABEL.fullmatch(version) is None:
        raise ValueError("release tag source requires SemVer vX.Y.Z")
    owner, repository = _github_action_repository(action_key)
    return (
        f"https://api.github.com/repos/{owner}/{repository}/commits/{version}"
    )


def github_latest_release_sources(action_key):
    """Return allowed same-repository latest release evidence endpoints."""
    owner, repository = _github_action_repository(action_key)
    root = f"https://api.github.com/repos/{owner}/{repository}"
    return tuple(f"{root}/{path}" for path in LATEST_RELEASE_PATHS)


def validate_action_pin(entry):
    """Return an error string unless inventory identity and mode agree."""
    if not isinstance(entry, dict):
        return "action pin entry must be an object"

    pin_kind = entry.get("pin_kind")
    if pin_kind is None:
        return "missing explicit pin_kind"
    if pin_kind not in ("release", "commit"):
        return f"unsupported pin_kind: {pin_kind!r}"

    version = entry.get("pinned_version")
    if pin_kind == "release":
        if not isinstance(version, str) or SEMVER_LABEL.fullmatch(version) is None:
            return "release pin label must be SemVer vX.Y.Z"
        sha = entry.get("pinned_sha")
        if not isinstance(sha, str) or FULL_SHA.fullmatch(sha) is None:
            return "release pin requires a full lowercase pinned_sha"
        if entry.get("qualified_version") != version \
                or entry.get("qualified_sha") != sha:
            return "qualified release identity must match pinned version and SHA"
        try:
            expected_source = github_tag_commit_source(entry.get("key"), version)
            latest_sources = github_latest_release_sources(entry.get("key"))
        except (AttributeError, ValueError) as error:
            return f"invalid release action key: {error}"
        if entry.get("source") != expected_source:
            return f"release source must be exact tag endpoint {expected_source}"
        if entry.get("latest_source") not in latest_sources:
            return "release latest_source must be an exact endpoint for its repository"
        latest = entry.get("latest")
        if not isinstance(latest, str) or SEMVER_LABEL.fullmatch(latest) is None:
            return "release latest evidence must be SemVer vX.Y.Z"
        return None

    sha = entry.get("pinned_sha")
    if not isinstance(sha, str) or FULL_SHA.fullmatch(sha) is None:
        return "commit pin requires a full lowercase pinned_sha"
    if version != f"commit-{sha[:7]}":
        return "commit pin label must match pinned_sha prefix"
    try:
        expected_source = github_commit_source(entry.get("key"), sha)
    except (AttributeError, ValueError) as error:
        return f"invalid commit action key: {error}"
    if entry.get("source") != expected_source:
        return f"commit source must be exact endpoint {expected_source}"
    if entry.get("qualified_sha") != sha or entry.get("qualified_version") != version:
        return "qualified commit identity must match pinned SHA and label"
    if entry.get("latest") != sha:
        return "commit latest evidence must match pinned_sha"
    return None


def commit_sha_from_response(source, body):
    """Return SHA only for exact GitHub commit endpoint/body identity."""
    match = COMMIT_API.fullmatch(source) if isinstance(source, str) else None
    if match is None:
        return None
    expected_sha = match.group(3)
    try:
        payload = json.loads(body)
    except (TypeError, ValueError):
        return None
    if not isinstance(payload, dict):
        return None
    actual_sha = payload.get("sha")
    if not isinstance(actual_sha, str) or FULL_SHA.fullmatch(actual_sha) is None:
        return None
    return actual_sha if actual_sha == expected_sha else None


def release_tag_sha_from_response(source, body):
    """Return full commit SHA only for a valid GitHub vX.Y.Z tag endpoint."""
    if not isinstance(source, str) or TAG_COMMIT_API.fullmatch(source) is None:
        return None
    try:
        payload = json.loads(body)
    except (TypeError, ValueError):
        return None
    if not isinstance(payload, dict):
        return None
    sha = payload.get("sha")
    if not isinstance(sha, str) or FULL_SHA.fullmatch(sha) is None:
        return None
    return sha


def release_tag_matches(entry, body):
    """Bind exact tag response SHA to both reviewed release identities."""
    if validate_action_pin(entry) is not None \
            or entry.get("pin_kind") != "release":
        return False
    actual_sha = release_tag_sha_from_response(entry.get("source"), body)
    return actual_sha is not None and actual_sha == entry.get("pinned_sha") \
        and actual_sha == entry.get("qualified_sha")
