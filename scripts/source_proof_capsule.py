"""Replayable proof capsule for the reviewed Semver source revision."""

import base64
import json
import re

from source_semver_capsule import archive_proof, require, sha, git_object


OWNER_MANIFEST_SHA256 = "13139d97abbd474aa0c92c533b81516c0c5e9376dad7d0d88b8a8f8a14b5c71a"
PROJECTION_SHA256 = "1abc3546d95f0af4582bc468fa748df85932b6b2db026917bcc2dde028e8cd73"
INLINE_LIMIT = 1024 * 1024
RECEIPT_LIMIT = 4 * 1024 * 1024


def _entry(kind, name, digest, size):
    return {"kind": kind, "name": name, "original_mode": "0600",
            "sha256": digest, "size_bytes": size}


PROJECTION = {
    "Cargo.lock": _entry("source_member", "Cargo.lock", "34280e954f1a748a60d7338272d76e95706df89771efc5a4f9938e53cd40609b", 104110),
    "LICENSE-APACHE": _entry("source_member", "LICENSE-APACHE", "91687e47b87fadb95cd01f7a85028c6ba4fab03bddb7269d581ae7dd43de5b03", 11379),
    "LICENSE-MIT": _entry("source_member", "LICENSE-MIT", "23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3", 1023),
    "abi-source-binding.json": _entry("inline_bytes", "abi-source-binding.json", "faa0ace5ac67e2f5614b644da61eaab385118067cd41f6c8ea1b67324d68b621", 1047),
    "independent-final-review.json": _entry("inline_bytes", "independent-final-review.json", "81289a4b235426be284ffdff5f6b64e7f6d81da91c18a12f290ed48de7c06a4a", 1169),
    "license-audit.json": _entry("inline_bytes", "license-audit.json", "a2b70711e9fa1758a9bf282a3be6e0731adac0c751be46ed49f4874ef533f9d5", 3497),
    "local-qualification/semantic-trigger-final.log": _entry("inline_bytes", "local-qualification/semantic-trigger-final.log", "7c8b662fd1b0a05ebbbf1725de02ef4f89da0a2667e0264b659116d96ce5a949", 561),
    "local-qualification/semantic-trigger-invocation.json": _entry("inline_bytes", "local-qualification/semantic-trigger-invocation.json", "e9f376af64c322ff70ab6fbe600ece37b84049712d0586e9ae1f8f2a9f02aa38", 1960),
    "local-qualification/semantic-trigger-summary.json": _entry("inline_bytes", "local-qualification/semantic-trigger-summary.json", "a233b41a57eb880df755b67832ac1707ab3fd38368f8111fafd19a55a2a005f1", 1634),
    "local-qualification/trigger-build-after.json": _entry("inline_bytes", "local-qualification/trigger-build-after.json", "f0e85b2eefd1a3990b8f86572e4ce33f361249c7495130c29f2b182874c9317e", 4459),
    "local-qualification/trigger-build-before.json": _entry("inline_bytes", "local-qualification/trigger-build-before.json", "176ca77a2ffa9d8e1cc45094aefe43402f980787073e43639830292d605aef7e", 331560),
    "local-qualification/trigger-build-invocation.json": _entry("inline_bytes", "local-qualification/trigger-build-invocation.json", "180258eab117c95a2be4b3d2621be9856efc47b3f727a3cd53495a8a7badaa57", 1216),
    "local-qualification/trigger-build.log": _entry("inline_bytes", "local-qualification/trigger-build.log", "ce6d7161839c53f3f6cbc2cc131ea06fc9f3b18621717aa1c934eeaf421b96fd", 72),
    "owned-semver-supplied-v1.patch": _entry("canonical_asset", "base.patch", "ae016d81b76419c9d96499c4f527a69867faa776884891d7a391290246708aef", 42058),
    "raw-commits/4297e8b5f6306531375ba2ba332171e5792b4c38.commit": _entry("inline_bytes", "raw-commits/4297e8b5f6306531375ba2ba332171e5792b4c38.commit", "7ac7acadff9b896171bd1836a79cf02d3ff5a7bb98722b7505cc2238505166ef", 1141),
    "raw-commits/583dddce84706786fc54c41a2c768c28a09c65fd.commit": _entry("canonical_asset", "source.commit", "8c4b69df1d8b56c165ed2e9a5a057969e6de58487e8597006bb5889a5f730d36", 351),
    "raw-commits/d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a.commit": _entry("inline_bytes", "raw-commits/d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a.commit", "3b2f8686c81780c271ab567f7962bca269e547ffce6f6c6e44232a90f3b95bab", 362),
    "source-file-inventory.json": _entry("inline_bytes", "source-file-inventory.json", "9c1154e9da0f79d568f82514620f435a83e873b3763120839aad34e56f46b021", 546821),
    "source-reconstruction-review.json": _entry("inline_bytes", "source-reconstruction-review.json", "3a24808738be8f39be205499356cd185a04849074616ce3155654b2964278749", 3449),
    "source.tar": _entry("canonical_asset", "source.tar", "38573667b13c541e368395259545be0be8f6858ada3b9c158ba93d8633b38c11", 7290880),
    "trigger-review.json": _entry("inline_bytes", "trigger-review.json", "2b12a8a3699624d7d2a83c386316a3e6a02df7b4bc6c1cc057bc0e3f2f6a6083", 1675),
}

_RECEIPT_FIELDS = {"schema", "status", "tool", "archive_kind", "upstream_repository",
    "upstream_base_commit", "source_commit", "source_tree", "source_archive", "base_patch",
    "lockfile", "license_files", "required_hosts", "proof_capsule", "publication",
    "behavioral_qualification", "signed_build_provenance", "sdk_qualification",
    "runtime_qualification", "three_host_qualification"}
_CAPSULE_FIELDS = {"schema", "original_manifest_sha256", "original_manifest_b64",
                   "projection", "proof_bytes"}
PROJECTION_DOCUMENT = {"assets": PROJECTION,
    "counts": {"canonical_asset": 3, "inline_bytes": 15, "source_member": 3},
    "decoded_inline_bytes": 900623, "original_manifest_sha256": OWNER_MANIFEST_SHA256}


def strict_json(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique)


def _decode(value, label):
    require(isinstance(value, str) and value and "\n" not in value and " " not in value,
            label + " base64 is not canonical")
    try:
        decoded = base64.b64decode(value, validate=True)
    except (ValueError, TypeError) as error:
        raise ValueError(label + " base64 is invalid") from error
    require(base64.b64encode(decoded).decode("ascii") == value, label + " base64 is not canonical")
    return decoded


def _verify_owner_manifest(raw, revision):
    require(len(raw) <= RECEIPT_LIMIT, "owner manifest exceeds bound")
    require(sha(raw) == OWNER_MANIFEST_SHA256, "owner manifest digest differs")
    manifest = strict_json(raw)
    require(type(manifest.get("schema_version")) is int and manifest.get("schema_version") == 1 and
            manifest.get("status") == "qualified_SOURCE" and
            manifest.get("commit") == revision.source_commit and manifest.get("tree") == revision.source_tree and
            manifest.get("source_archive_sha256") == revision.source_archive_sha256 and
            manifest.get("patch_sha256") == revision.base_patch_sha256 and
            manifest.get("runtime_binary_asset_included") is False and
            manifest.get("sdk_qualified") is False and manifest.get("runtime_qualified") is False and
            manifest.get("three_host_qualified") is False and manifest.get("remote_publication_performed") is False,
            "owner manifest qualification scope differs")
    assets = manifest.get("assets")
    require(isinstance(assets, dict) and set(assets) == set(PROJECTION),
            "owner manifest asset closure differs")
    for name, expected in PROJECTION.items():
        actual = assets[name]
        require(isinstance(actual, dict) and
                actual.get("sha256") == expected["sha256"] and
                actual.get("size_bytes") == expected["size_bytes"] and
                actual.get("mode") == expected["original_mode"],
                "owner manifest asset metadata differs")
    return manifest


def _verify_commits(proof_bytes, final_raw, revision):
    raw = {name: _decode(data, name) for name, data in proof_bytes.items()}
    raw["raw-commits/583dddce84706786fc54c41a2c768c28a09c65fd.commit"] = final_raw
    identities = {
        "raw-commits/4297e8b5f6306531375ba2ba332171e5792b4c38.commit":
            ("4297e8b5f6306531375ba2ba332171e5792b4c38", "4f640b40228b0162141eb1d10961b4c1b5029189", ["07b168194b0e7dc59d9e462813008d93292dce5a"]),
        "raw-commits/d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a.commit":
            ("d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a", "0737734a15375f7fc4d4314522cae8f82cf47813", ["4297e8b5f6306531375ba2ba332171e5792b4c38"]),
        "raw-commits/583dddce84706786fc54c41a2c768c28a09c65fd.commit":
            (revision.source_commit, revision.source_tree, ["d73a5d2469f3cf2a8e77cb3259ae4da15a7c9d0a"]),
    }
    for name, (commit, tree, parents) in identities.items():
        data = raw[name]
        require(git_object("commit", data) == commit, "raw commit object differs")
        headers, message = data.split(b"\n\n", 1)
        lines = headers.decode("utf-8").splitlines()
        require(lines[0] == "tree " + tree and
                [line[7:] for line in lines if line.startswith("parent ")] == parents,
                "raw commit tree or parent chain differs")
        if commit != "4297e8b5f6306531375ba2ba332171e5792b4c38":
            text = message.decode("utf-8")
            require(re.search(r"^Signed-off-by: .+ <[^<>\n]+>$", text, re.MULTILINE) and
                    "Co-authored-by: Codex <codex@openai.com>" in text,
                    "owned raw commit trailers missing")
    final_name = "raw-commits/583dddce84706786fc54c41a2c768c28a09c65fd.commit"
    final_metadata = PROJECTION[final_name]
    require(len(final_raw) == final_metadata["size_bytes"] and
            sha(final_raw) == final_metadata["sha256"],
            "final raw commit proof bytes differ")


def validate_semver_receipt(receipt, receipt_bytes, assets, revision, git_files):
    """Validate the typed receipt and return archive members for source proof."""
    require(len(receipt_bytes) <= RECEIPT_LIMIT and isinstance(receipt, dict) and
            set(receipt) == _RECEIPT_FIELDS, "semver source receipt fields must be closed")
    require(type(receipt["schema"]) is int and receipt["schema"] == 2 and
            receipt["status"] == "STAGED_SOURCE_ONLY" and
            receipt["tool"] == revision.role.value and receipt["archive_kind"] == revision.archive_kind.value and
            receipt["upstream_repository"] == "https://github.com/" + revision.upstream_repository and
            receipt["upstream_base_commit"] == revision.upstream_base_commit and
            receipt["source_commit"] == revision.source_commit and receipt["source_tree"] == revision.source_tree and
            receipt["required_hosts"] == [] and receipt["publication"] is None and
            receipt["behavioral_qualification"] is None and receipt["signed_build_provenance"] is None and
            receipt["sdk_qualification"] is None and receipt["runtime_qualification"] is None and
            receipt["three_host_qualification"] is None, "semver source receipt claim differs")
    _verify_asset_record(receipt["source_archive"], "source.tar", assets["source.tar"], revision.source_archive_sha256, revision.source_archive_size)
    _verify_asset_record(receipt["base_patch"], "base.patch", assets["base.patch"], revision.base_patch_sha256, None)
    require(receipt["lockfile"] == {"path": "Cargo.lock", "sha256": PROJECTION["Cargo.lock"]["sha256"]},
            "semver lockfile proof differs")
    require(receipt["license_files"] == {name: PROJECTION[name]["sha256"] for name in ("LICENSE-APACHE", "LICENSE-MIT")},
            "semver license proof differs")
    capsule = receipt["proof_capsule"]
    require(isinstance(capsule, dict) and set(capsule) == _CAPSULE_FIELDS and
            type(capsule["schema"]) is int and capsule["schema"] == 1,
            "semver proof capsule fields must be closed")
    require(capsule["original_manifest_sha256"] == OWNER_MANIFEST_SHA256 and
            capsule["projection"] == PROJECTION_DOCUMENT and
            sha((json.dumps(PROJECTION_DOCUMENT, indent=2, sort_keys=True) + "\n").encode()) == PROJECTION_SHA256,
            "semver proof projection differs")
    owner_raw = _decode(capsule["original_manifest_b64"], "owner manifest")
    _verify_owner_manifest(owner_raw, revision)
    inline = {name: item for name, item in PROJECTION.items() if item["kind"] == "inline_bytes"}
    proof_bytes = capsule["proof_bytes"]
    require(isinstance(proof_bytes, dict) and set(proof_bytes) == set(inline),
            "semver inline proof set differs")
    total = 0
    for name, metadata in inline.items():
        data = _decode(proof_bytes[name], name)
        require(len(data) == metadata["size_bytes"] and sha(data) == metadata["sha256"],
                "semver inline proof bytes differ")
        total += len(data)
    require(total == 900623 and total <= INLINE_LIMIT, "semver inline proof bound differs")
    _verify_commits(proof_bytes, assets["source.commit"], revision)
    inventory = strict_json(_decode(proof_bytes["source-file-inventory.json"], "source inventory"))
    members = archive_proof(assets["source.tar"], inventory, revision, git_files)
    for name in ("Cargo.lock", "LICENSE-APACHE", "LICENSE-MIT"):
        data = members[name]
        metadata = PROJECTION[name]
        require(len(data) == metadata["size_bytes"] and sha(data) == metadata["sha256"],
                "semver source member proof differs")
    return members


def _verify_asset_record(record, name, data, expected_sha, expected_size):
    require(isinstance(record, dict) and set(record) == {"name", "sha256", "size_bytes"} and
            record["name"] == name and record["sha256"] == expected_sha and
            type(record["size_bytes"]) is int and
            len(data) == record["size_bytes"] and (expected_size is None or record["size_bytes"] == expected_size) and
            sha(data) == record["sha256"], name + " source asset proof differs")
