"""Native APT stage and deploy guard; fixed inputs and fail-closed publication."""
import os
import re
from pathlib import Path

from delivery_apt_core import config_digest, digest, read_json, regular, require, write_json
from delivery_apt_stage_feed import ARCHES, candidate_hashes, live_suite, local_suite, names
from delivery_apt_stage_publish import publish, version_order
from delivery_apt_verify import signer, validate_config, version_parts


PROOF = ".apt-stage.json"


def inventory(root, excluded=()):
    require(root.is_dir() and not root.is_symlink(), "artifact root is not a real directory")
    files = {}
    for path in sorted(root.rglob("*")):
        require(not path.is_symlink(), "artifact symlink forbidden")
        if path.is_dir():
            continue
        relative = path.relative_to(root).as_posix()
        regular(path)
        if relative not in excluded:
            files[relative] = digest(path)
    return files


def bind_run(marker):
    for field, env in (("workflow_run_id", "GITHUB_RUN_ID"), ("workflow_run_attempt", "GITHUB_RUN_ATTEMPT")):
        require(marker.get(field) == os.environ.get(env, ""), "artifact belongs to another workflow run")
    sha = os.environ.get("GITHUB_SHA", "")
    require(re.fullmatch(r"[0-9a-f]{40}", sha) and marker.get("consumer_source_sha") == sha,
            "artifact belongs to another consumer source SHA")


def verified_input(config):
    marker = read_json("incoming/.apt-verified.json")
    require(marker.get("schema") == "velnor.apt-verified/v1" and
            marker.get("config") == config and marker.get("config_sha256") == config_digest(config),
            "verified artifact configuration mismatch")
    bind_run(marker)
    suite = marker.get("suite")
    require(suite in ("stable", "preview") and suite == os.environ.get("INPUT_SUITE", "stable"),
            "verified suite mismatch")
    version, base = version_parts(suite, marker.get("version", ""))
    require(marker.get("debian_version") == version, "verified version mismatch")
    require(inventory(Path("incoming"), (".apt-verified.json",)) == marker.get("files"),
            "verified artifact changed after verification")
    packages = marker.get("packages", [])
    require(len(packages) == 2 and {item.get("arch") for item in packages} == set(ARCHES),
            "verified package pair incomplete")
    for item in packages:
        require(marker["files"].get(item["name"]) == item["sha256"], "verified package digest mismatch")
    manifest = "manifest.json" if suite == "stable" else "release-manifest.json"
    require(marker["files"].get(manifest) == marker.get("manifest_sha256"), "verified manifest digest mismatch")
    source = "release-record.json" if suite == "stable" else manifest
    require(marker["files"].get(source) == marker.get("source_record_sha256"), "verified source digest mismatch")
    return marker


def stage(config):
    validate_config(config)
    signer(config)
    marker = verified_input(config)
    suite = marker["suite"]
    live = live_suite(config, suite, absent=(suite == "preview"))
    other = live_suite(config, "preview" if suite == "stable" else "stable", absent=True)
    root = publish(config, marker, live, other)
    proof = {"schema": "velnor.apt-stage/v1", "config_sha256": config_digest(config),
             "suite": suite, "version": marker["version"], "debian_version": marker["debian_version"],
             "commit": marker["commit"], "live_record": live["record"] if live is not None else None,
             "other_record": other["record"] if other is not None else None,
             "source_ref": marker["source_ref"], "source_record_sha256": marker["source_record_sha256"],
             "manifest_sha256": marker["manifest_sha256"], "packages": marker["packages"],
             "workflow_run_id": marker["workflow_run_id"],
             "workflow_run_attempt": marker["workflow_run_attempt"],
             "consumer_source_sha": marker["consumer_source_sha"], "files": inventory(root)}
    write_json(root / PROOF, proof)


def guard(config):
    validate_config(config)
    root = Path("public")
    proof = read_json(root / PROOF)
    require(proof.get("schema") == "velnor.apt-stage/v1" and
            proof.get("config_sha256") == config_digest(config), "staged configuration mismatch")
    bind_run(proof)
    require(inventory(root, (PROOF,)) == proof.get("files"), "staged artifact corrupted")
    signer(config)
    signer(dict(config, keyring=str(root / Path(config["keyring"]).name)))
    suite = proof.get("suite")
    require(suite in ("stable", "preview") and suite == os.environ.get("INPUT_SUITE", "stable"),
            "staged suite mismatch")
    version, base = version_parts(suite, proof.get("version", ""))
    require(version == proof.get("debian_version"), "staged version mismatch")
    record_name, pointer_name, state_name = names(suite)
    record = local_suite(config, suite, root)
    require(record.get("crate_version") == version and
            (root / pointer_name).read_text().strip() == proof["version"], "staged pointer mismatch")
    state = read_json(root / state_name)
    require(state.get("source_commit") == proof["commit"] and state.get("source_ref") == proof.get("source_ref"),
            "staged commit/ref mismatch")
    require(record.get("source_record_sha256") == proof.get("source_record_sha256"), "staged source digest mismatch")
    expected_packages = sorted([{ "name": item["name"], "sha256": item["sha256"]}
                                for item in proof.get("packages", [])], key=lambda item: item["name"])
    require(state.get("packages") == expected_packages, "staged candidate packages mismatch")
    candidate = candidate_hashes(suite, root, version)
    packages = proof.get("packages", [])
    require(len(packages) == 2 and {item.get("arch") for item in packages} == set(ARCHES),
            "staged candidate architecture proof mismatch")
    require({item["arch"]: item["sha256"] for item in packages} == candidate,
            "staged candidate proof differs from signed indexes")
    live = live_suite(config, suite, absent=True)
    require(live is not None or proof.get("live_record") is None, "live suite disappeared after staging")
    require(suite == "preview" or live is not None, "stable live suite cannot bootstrap")
    if live is not None:
        published = live["record"]
        order = version_order(version, published["crate_version"], suite)
        require(order >= 0, "staged version older than live; refusing rollback")
        if order > 0:
            require(published == proof.get("live_record"), "live rollback head changed after staging; rebuild required")
        if order == 0:
            require(record["source_record_sha256"] == published["source_record_sha256"],
                    "equal live version has different immutable source")
            for arch in ARCHES:
                entries = [item for item in live["entries"][arch] if item["Version"] == version]
                require(len(entries) == 1 and entries[0]["SHA256"] == candidate[arch],
                        "equal live version has different candidate package bytes")
    other_suite = "preview" if suite == "stable" else "stable"
    other_record = root / names(other_suite)[0]
    require(other_record.exists() == (proof.get("other_record") is not None), "other suite presence proof mismatch")
    if other_record.exists():
        preserved = local_suite(config, other_suite, root)
        require(preserved == proof["other_record"], "preserved other suite proof mismatch")
        other = live_suite(config, other_suite, absent=True)
        require(other is not None, "other suite disappeared after staging")
        if other is not None:
            require(other["record"] == preserved, "other suite advanced after staging")
    else:
        require(live_suite(config, other_suite, absent=True) is None, "other suite appeared after staging")
