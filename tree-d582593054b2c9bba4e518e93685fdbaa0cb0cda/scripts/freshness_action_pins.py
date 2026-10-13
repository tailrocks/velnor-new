"""Fail-closed validation for release and immutable commit action pins."""

import json
import re

FULL_SHA = re.compile(r"[0-9a-f]{40}\Z")
SEMVER_LABEL = re.compile(r"v[0-9]+\.[0-9]+\.[0-9]+\Z")
COMMIT_API = re.compile(
    r"https://api\.github\.com/repos/([A-Za-z0-9_.-]+)/"
    r"([A-Za-z0-9_.-]+)/commits/([0-9a-f]{40})\Z"
)


def github_commit_source(action_key, sha):
    """Return the repository commit endpoint, stripping any action subpath."""
    if not isinstance(sha, str) or FULL_SHA.fullmatch(sha) is None:
        raise ValueError("commit API source requires a full lowercase SHA")
    parts = action_key.split("/") if isinstance(action_key, str) else []
    if len(parts) < 2 or not all(parts[:2]):
        raise ValueError("action key must include owner and repository")
    if any(re.fullmatch(r"[A-Za-z0-9_.-]+", part) is None for part in parts):
        raise ValueError("action key contains an invalid path segment")
    owner, repository = parts[:2]
    return (
        f"https://api.github.com/repos/{owner}/{repository}/commits/{sha}"
    )


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
