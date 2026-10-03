#!/usr/bin/env python3
"""Publish five immutable source-only assets for a closed reviewed revision."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

from source_publication import REPO, git_object, prepare, require, sha, source_identity, strict_json
from source_publication_records import ArchiveKind, role_names
from source_release_policy import verify_protection

PREFIX = "repos/" + REPO + "/"


def gh(*arguments):
    environment = dict(os.environ, GH_HOST="github.com", GH_REPO=REPO, GH_PROMPT_DISABLED="1")
    result = subprocess.run(["gh", *arguments], capture_output=True, check=False, env=environment)
    if result.returncode:
        raise subprocess.CalledProcessError(result.returncode, result.args, result.stdout, result.stderr)
    return result.stdout


def api(endpoint):
    return strict_json(gh("api", PREFIX.rstrip("/") if not endpoint else PREFIX + endpoint))


def immutable_enabled():
    require(api("immutable-releases").get("enabled") is True,
            "immutable releases must already be enabled")


def absent(endpoint):
    try:
        gh("api", PREFIX + endpoint)
    except subprocess.CalledProcessError as error:
        require(re.search(rb"\(HTTP 404\)", error.stderr or b"") is not None,
                "existence check did not return authenticated HTTP 404")
        return
    raise ValueError("source publication tag or release already exists")


def source_authority(manifest, revision):
    ref = api("git/ref/" + revision.source_ref[len("refs/"):])
    require(ref.get("ref") == revision.source_ref and
            ref.get("object", {}).get("type") == "commit" and
            ref["object"].get("sha") == revision.source_commit, "owned source ref changed")
    commit = api("git/commits/" + revision.source_commit)
    require(commit.get("sha") == revision.source_commit and
            commit.get("tree", {}).get("sha") == revision.source_tree, "owned source API tree changed")
    comparison = api("compare/" + revision.upstream_base_commit + "..." + revision.source_commit)
    require(comparison.get("status") == "ahead" and type(comparison.get("behind_by")) is int and
            comparison["behind_by"] == 0 and
            comparison.get("merge_base_commit", {}).get("sha") == revision.upstream_base_commit and
            type(comparison.get("ahead_by")) is int and comparison["ahead_by"] > 0,
            "owned source API ancestry differs from reviewed base")


def generator_latest():
    latest = api("releases/latest")
    require(type(latest.get("id")) is int and latest["id"] > 0 and
            isinstance(latest.get("tag_name"), str) and
            not latest["tag_name"].startswith(("owned-source-", "mise-v", "mbx-v")),
            "owned source/tool releases cannot become generator latest")


def authority(manifest, revision):
    branch = api("branches/main")
    require(branch.get("name") == "main" and branch.get("protected") is True and
            branch.get("commit", {}).get("sha") == revision.tag_target and
            manifest["tag_target"] == revision.tag_target,
            "protected main differs from reviewed generator target")
    verify_protection(api, revision)
    immutable_enabled()
    generator_latest()
    source_authority(manifest, revision)


def tag_target(manifest, revision):
    ref = api("git/ref/tags/" + revision.tag)
    require(ref.get("ref") == "refs/tags/" + revision.tag and
            ref.get("object", {}).get("type") == "commit" and
            ref["object"].get("sha") == revision.tag_target, "source release tag changed target")


def release_state(manifest, revision, release_id, asset_ids, draft, assets=None):
    release = api("releases/" + str(release_id))
    require(release.get("id") == release_id and type(release.get("id")) is int and
            release.get("tag_name") == revision.tag and
            release.get("target_commitish") == revision.tag_target and
            release.get("draft") is draft and release.get("prerelease") is False,
            "source release identity or state changed")
    records = release.get("assets")
    require(isinstance(records, list) and len(records) == 5 and
            all(isinstance(asset, dict) and type(asset.get("id")) is int and asset["id"] > 0
                and isinstance(asset.get("name"), str) for asset in records) and
            {asset["name"]: asset["id"] for asset in records} == asset_ids and
            len(set(asset_ids.values())) == 5, "source release assets changed")
    if assets is not None:
        for record in records:
            require(record.get("digest") == "sha256:" + sha(assets[record["name"]]),
                    "source release asset digest differs")
    if not draft:
        require(release.get("immutable") is True, "published source release is not immutable")
    tag_target(manifest, revision)


def download_assets(assets, asset_ids):
    for name, asset_id in asset_ids.items():
        downloaded = gh("api", PREFIX + "releases/assets/" + str(asset_id),
                        "--header", "Accept: application/octet-stream")
        require(len(downloaded) == len(assets[name]) and sha(downloaded) == sha(assets[name]),
                "downloaded source release bytes differ from reviewed asset")


def publication_input(manifest, assets):
    fields = "schema status tool source_ref source_commit source_tree upstream_base_commit tag tag_target repository source_receipt raw_commit assets behavioral_qualification signed_build_provenance".split()
    require(isinstance(manifest, dict) and set(manifest) == set(fields),
            "source publication manifest fields must be closed")
    revision = source_identity(manifest["tool"], manifest["source_ref"],
                               manifest["source_commit"], manifest["tag_target"])
    require(set(assets) == {"source.tar", "base.patch", "source-receipt.json",
                           "source.commit", "source-publication.json"}, "source asset set mismatch")
    require(manifest["tag"] == revision.tag and manifest["status"] == "SOURCE_ONLY" and
            manifest["source_tree"] == revision.source_tree and manifest["repository"] == REPO and
            manifest["behavioral_qualification"] is None and manifest["signed_build_provenance"] is None,
            "source publication claim mismatch")
    require(type(manifest["schema"]) is int and manifest["schema"] == 1 and
            manifest["upstream_base_commit"] == revision.upstream_base_commit,
            "source publication schema/base mismatch")
    raw = assets["source.commit"]
    message = raw.split(b"\n\n", 1)[1].decode()
    expected_raw = {"sha256": sha(raw), "git_object_sha1": revision.source_commit,
        "dco_signoffs": re.findall(r"^Signed-off-by: (.+ <[^<>\n]+>)$", message, re.MULTILINE),
        "cryptographic_signature": None}
    expected_assets = {name: {"sha256": sha(data), "size": len(data)}
                       for name, data in assets.items() if name != "source-publication.json"}
    require(json.dumps(manifest["raw_commit"], sort_keys=True) == json.dumps(expected_raw, sort_keys=True) and
            json.dumps(manifest["assets"], sort_keys=True) == json.dumps(expected_assets, sort_keys=True),
            "raw commit/DCO or source asset manifest differs from actual bytes")
    require(git_object("commit", raw) == revision.source_commit and
            sha(raw) == revision.raw_commit_sha256,
            "raw source commit differs from the reviewed Git object")
    require(sha(assets["source.tar"]) == revision.source_archive_sha256 and
            len(assets["source.tar"]) == revision.source_archive_size and
            sha(assets["base.patch"]) == revision.base_patch_sha256 and
            sha(assets["source-receipt.json"]) == revision.source_receipt_sha256 and
            json.dumps(manifest["source_receipt"], sort_keys=True) ==
            json.dumps(strict_json(assets["source-receipt.json"]), sort_keys=True) and
            assets["source-publication.json"] ==
            (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode(),
            "source publication proof asset mismatch")
    if revision.archive_kind is ArchiveKind.GIT_TAR_UMASK_022_V1:
        from source_proof_capsule import validate_semver_receipt
        validate_semver_receipt(manifest["source_receipt"], assets["source-receipt.json"],
                                assets, revision, None)
    return revision


def create_draft(manifest, revision):
    payload = {"tag_name": revision.tag, "target_commitish": revision.tag_target,
        "name": revision.tag, "draft": True, "prerelease": False,
        "generate_release_notes": False, "make_latest": "false",
        "body": "Reviewed source bytes and Git object/DCO evidence only; behavioral qualification and signed build provenance are absent."}
    with tempfile.TemporaryDirectory(prefix="source-draft-request-") as temporary:
        body = Path(temporary) / "release.json"
        body.write_text(json.dumps(payload), encoding="utf-8")
        created = strict_json(gh("api", "--method", "POST", PREFIX + "releases", "--input", str(body)))
    release_id = created.get("id")
    require(type(release_id) is int and release_id > 0 and created.get("draft") is True and
            created.get("prerelease") is False and created.get("tag_name") == revision.tag and
            created.get("target_commitish") == revision.tag_target and created.get("assets") == [],
            "release creation response does not identify the new empty draft")
    return release_id


def asset_identity(manifest, revision, assets, release_id):
    release = api("releases/" + str(release_id))
    records = release.get("assets")
    require(isinstance(records, list) and len(records) == 5 and
            all(isinstance(record, dict) and isinstance(record.get("name"), str) and
                type(record.get("id")) is int and record["id"] > 0 for record in records) and
            {record["name"] for record in records} == set(assets), "source draft asset identity mismatch")
    asset_ids = {record["name"]: record["id"] for record in records}
    release_state(manifest, revision, release_id, asset_ids, True, assets)
    return asset_ids


def promote(manifest, revision, assets, release_id, asset_ids):
    download_assets(assets, asset_ids)
    authority(manifest, revision)
    release_state(manifest, revision, release_id, asset_ids, True, assets)
    return finish_promotion(manifest, revision, release_id, asset_ids)


def finish_promotion(manifest, revision, release_id, asset_ids):
    gh("api", "--method", "PATCH", PREFIX + "releases/" + str(release_id),
       "--field", "draft=false", "--raw-field", "make_latest=false")
    release_state(manifest, revision, release_id, asset_ids, False)
    authority(manifest, revision)
    return {"status": "PUBLISHED_SOURCE_ONLY", "tag": revision.tag, "release_id": release_id,
            "source_commit": revision.source_commit, "qualified": False}


def publish(manifest, assets):
    revision = publication_input(manifest, assets)
    authority(manifest, revision)
    absent("git/ref/tags/" + revision.tag)
    absent("releases/tags/" + revision.tag)
    gh("api", "--method", "POST", PREFIX + "git/refs", "--field", "ref=refs/tags/" + revision.tag,
       "--field", "sha=" + revision.tag_target)
    tag_target(manifest, revision)
    release_id = create_draft(manifest, revision)
    with tempfile.TemporaryDirectory(prefix="owned-source-publication-") as temporary:
        snapshot = Path(temporary).resolve()
        for name, data in sorted(assets.items()):
            path = snapshot / name
            path.write_bytes(data)
            gh("release", "upload", revision.tag, str(path), "--repo", REPO)
    asset_ids = asset_identity(manifest, revision, assets, release_id)
    return promote(manifest, revision, assets, release_id, asset_ids)


RECOVERY_ID = 402229309
RECOVERY_ASSETS = {"base.patch": 606739719, "source-publication.json": 606739767,
    "source-receipt.json": 606739819, "source.commit": 606739866, "source.tar": 606739944}
RECOVERY_MANIFEST_SHA256 = "d384bf993490548ff92042ba72c31d7acf4a2e34f725e9d2fda47c621bc28dc6"


def verify_retained_mise_draft(manifest, assets):
    revision = publication_input(manifest, assets)
    require(revision.role.value == "mise" and sha(assets["source-publication.json"]) == RECOVERY_MANIFEST_SHA256,
            "recovery requires the exact retained reviewed Mise publication manifest")
    authority(manifest, revision)
    release_state(manifest, revision, RECOVERY_ID, RECOVERY_ASSETS, True)
    download_assets(assets, RECOVERY_ASSETS)
    authority(manifest, revision)
    release_state(manifest, revision, RECOVERY_ID, RECOVERY_ASSETS, True)
    return revision


def recover_retained_mise_draft(manifest, assets):
    revision = verify_retained_mise_draft(manifest, assets)
    return finish_promotion(manifest, revision, RECOVERY_ID, RECOVERY_ASSETS)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", required=True, type=Path)
    parser.add_argument("--repository", required=True, type=Path)
    parser.add_argument("--role", required=True, choices=role_names())
    parser.add_argument("--source-ref", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--recover-retained-mise", action="store_true",
                        help="promote only the already uploaded reviewed draft 402229309")
    arguments = parser.parse_args()
    try:
        manifest, assets = prepare(arguments.stage, arguments.repository, arguments.role,
            arguments.source_ref, arguments.source_commit, arguments.target)
        result = (recover_retained_mise_draft(manifest, assets) if arguments.recover_retained_mise
                  else publish(manifest, assets))
    except (ValueError, OSError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        raise SystemExit("source-only publication failed: " + str(error)) from error
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
