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
    (snapshot / "owned-tool-manifest.json").write_bytes(canonical)
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


def asset_ids(assets, names):
    require(isinstance(assets, list) and len(assets) == len(names) and
            all(isinstance(asset, dict) and isinstance(asset.get("name"), str) and
                type(asset.get("id")) is int and asset["id"] > 0 for asset in assets) and
            {asset["name"] for asset in assets} == set(names), "release asset set or typed IDs mismatch")
    result = {asset["name"]: asset["id"] for asset in assets}
    require(len(set(result.values())) == len(result), "duplicate release asset IDs")
    return result


def verify_release(gh, manifest, release_id, expected, draft):
    release = decode(gh("api", PREFIX + "releases/" + str(release_id)))
    assets = release.get("assets")
    require(type(release.get("id")) is int and release["id"] == release_id and
            release.get("target_commitish") == manifest["workflow_commit"] and
            release.get("prerelease") is False and release.get("draft") is draft and
            release.get("tag_name") == manifest["tag"] and asset_ids(assets, expected) == expected,
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


def create_draft(snapshot, manifest, gh):
    payload = {"tag_name": manifest["tag"], "target_commitish": manifest["workflow_commit"],
        "name": manifest["tag"], "draft": True, "prerelease": False,
        "generate_release_notes": False, "make_latest": "false", "body": "Owned source build artifacts with associated signed evidence."}
    path = snapshot / ".release-create.json"
    path.write_text(json.dumps(payload), encoding="utf-8")
    try:
        created = decode(gh("api", "--method", "POST", PREFIX + "releases", "--input", str(path)))
    finally:
        path.unlink()
    require(type(created.get("id")) is int and created["id"] > 0 and
            created.get("draft") is True and created.get("prerelease") is False and
            created.get("tag_name") == manifest["tag"] and
            created.get("target_commitish") == manifest["workflow_commit"] and created.get("assets") == [],
            "release creation must return the exact new empty draft identity")
    return created["id"]


def generator_latest(gh):
    latest = decode(gh("api", PREFIX + "releases/latest"))
    require(type(latest.get("id")) is int and latest["id"] > 0 and
            isinstance(latest.get("tag_name"), str) and
            not latest["tag_name"].startswith(("owned-source-", "mise-v", "mbx-v")),
            "owned source/tool releases cannot become generator latest")


def publish_snapshot(snapshot, manifest, gh, verify, validate):
    immutable_enabled(gh)
    generator_latest(gh)
    tag = manifest["tag"]
    require_absent(gh, PREFIX + "git/ref/tags/" + tag)
    require_absent(gh, PREFIX + "releases/tags/" + tag)
    gh("api", "--method", "POST", PREFIX + "git/refs",
       "--raw-field", "ref=refs/tags/" + tag, "--raw-field", "sha=" + manifest["workflow_commit"])
    release_id = create_draft(snapshot, manifest, gh)
    verify_tag(gh, manifest)
    names = sorted(path.name for path in snapshot.iterdir())
    for name in names:
        gh("release", "upload", tag, str(snapshot / name), "--repo", REPO)
    release = decode(gh("api", PREFIX + "releases/" + str(release_id)))
    require(release.get("id") == release_id, "created release identity changed")
    expected = asset_ids(release.get("assets"), names)
    verify_release(gh, manifest, release_id, expected, True)
    verify_downloads(snapshot, manifest, gh, verify, validate, expected)
    immutable_enabled(gh)
    verify_release(gh, manifest, release_id, expected, True)
    gh("api", "--method", "PATCH", PREFIX + "releases/" + str(release_id),
       "--field", "draft=false", "--raw-field", "make_latest=false")
    verify_release(gh, manifest, release_id, expected, False)
    generator_latest(gh)
