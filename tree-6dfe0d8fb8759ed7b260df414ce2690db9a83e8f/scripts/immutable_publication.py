"""Durable evidence and GitHub immutable-release transaction helpers."""

import hashlib
import json
import re
import subprocess

from owned_tool_source import strict_json, validate_receipt
from owned_tool_qualification_evidence import stage_qualified_evidence

REPO = "tailrocks/velnor-new"
PREFIX = "repos/" + REPO + "/"
PREDICATE = "https://tailrocks.dev/owned-tool-qualification/v1"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def decode(data):
    return strict_json(data)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def immutable_enabled(gh):
    policy = decode(gh("api", PREFIX + "immutable-releases"))
    require(policy.get("enabled") is True, "GitHub immutable releases must already be enabled")


def require_absent(gh, endpoint):
    try:
        gh("api", endpoint)
    except subprocess.CalledProcessError as error:
        require(re.search(rb"\(HTTP 404\)", error.stderr or b"") is not None,
                "existence check failed without an authenticated 404")
        return
    raise ValueError("tag or release already exists")


def stage_evidence(snapshot, manifest, directory, read_regular, gh, verify):
    receipt = read_regular(directory / "source-receipt.json")
    source = manifest["source"]
    require(digest(receipt) == source["receipt_sha256"], "reviewed source receipt digest mismatch")
    spec = {"tool": manifest["tool"], "source_commit": source["commit"],
            "source_tree": source["tree"], "archive_sha256": source["archive_sha256"],
            "lockfile_sha256": source["lockfile_sha256"],
            "patch_sha256": source["base_patch_sha256"], "license_files": source["license_files"]}
    validate_receipt(receipt, spec)
    (snapshot / "source-receipt.json").write_bytes(receipt)
    stage_qualified_evidence(snapshot, manifest, directory, read_regular)
    canonical = (json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n").encode()
    (snapshot / "manifest.json").write_bytes(canonical)
    for artifact in manifest["artifacts"]:
        path = snapshot / artifact["name"]
        gh("attestation", "download", str(path), "--repo", REPO,
           "--predicate-type", PREDICATE, cwd=snapshot)
        generated = snapshot / ("sha256:" + artifact["archive_sha256"] + ".jsonl")
        bundle = snapshot / ("attestation-" + artifact["target"] + ".jsonl")
        data = read_regular(generated)
        require(data, "empty attestation bundle")
        bundle.write_bytes(data)
        generated.unlink()
        verify(path, manifest, artifact, bundle)


def verify_tag(gh, manifest):
    ref = decode(gh("api", PREFIX + "git/ref/tags/" + manifest["tag"]))
    object_ = ref.get("object", {})
    require(object_.get("type") == "commit" and
            object_.get("sha") == manifest["workflow_commit"],
            "release tag does not identify the trusted workflow commit")


def verify_release(gh, manifest, release_id, expected, draft):
    release = decode(gh("api", PREFIX + "releases/" + str(release_id)))
    assets = release.get("assets")
    require(release.get("id") == release_id and release.get("draft") is draft and
            release.get("tag_name") == manifest["tag"] and isinstance(assets, list) and
            len(assets) == len(expected) and {asset["name"]: asset["id"] for asset in assets} == expected,
            "release identity, tag, state or exact assets changed")
    if not draft:
        require(release.get("immutable") is True, "published release is not immutable")
    verify_tag(gh, manifest)


def verify_downloads(snapshot, manifest, gh, verify, validate, expected):
    artifacts = {artifact["name"]: artifact for artifact in manifest["artifacts"]}
    for name, asset_id in expected.items():
        require(type(asset_id) is int and asset_id > 0, "invalid release asset id")
        data = gh("api", PREFIX + "releases/assets/" + str(asset_id),
                  "--header", "Accept: application/octet-stream")
        require(digest(data) == digest((snapshot / name).read_bytes()),
                "downloaded release asset digest mismatch")
        (snapshot / ("download-" + name)).write_bytes(data)
        if name in artifacts:
            validate(data, artifacts[name], manifest["tool"], manifest["source"])
    for artifact in manifest["artifacts"]:
        verify(snapshot / ("download-" + artifact["name"]), manifest, artifact,
               snapshot / ("download-attestation-" + artifact["target"] + ".jsonl"))


def publish_snapshot(snapshot, manifest, gh, verify, validate):
    immutable_enabled(gh)
    tag = manifest["tag"]
    require_absent(gh, PREFIX + "git/ref/tags/" + tag)
    require_absent(gh, PREFIX + "releases/tags/" + tag)
    gh("release", "create", tag, "--repo", REPO, "--target",
       manifest["workflow_commit"], "--draft", "--title", tag,
       "--notes", "Owned source build artifacts with associated signed evidence.")
    verify_tag(gh, manifest)
    names = sorted(path.name for path in snapshot.iterdir())
    for name in names:
        gh("release", "upload", tag, str(snapshot / name), "--repo", REPO)
    release = decode(gh("api", PREFIX + "releases/tags/" + tag))
    release_id = release.get("id")
    require(type(release_id) is int and release_id > 0, "invalid release identity")
    assets = release.get("assets")
    require(isinstance(assets, list) and len(assets) == len(names) and
            {asset["name"] for asset in assets} == set(names), "release asset set mismatch")
    expected = {asset["name"]: asset["id"] for asset in assets}
    require(len(set(expected.values())) == len(expected), "duplicate release asset IDs")
    verify_release(gh, manifest, release_id, expected, True)
    verify_downloads(snapshot, manifest, gh, verify, validate, expected)
    immutable_enabled(gh)
    verify_release(gh, manifest, release_id, expected, True)
    gh("api", "--method", "PATCH", PREFIX + "releases/" + str(release_id),
       "--field", "draft=false")
    verify_release(gh, manifest, release_id, expected, False)
