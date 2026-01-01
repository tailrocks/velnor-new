#!/usr/bin/env python3
"""Publish a new owned-tool release from three authenticated native archives.

Closed manifest schema (unknown keys rejected):
{schema:1, tool:"mise"|"mbx", version:"X.Y.Z-owned-SUFFIX",
 tag:"TOOL-vVERSION", workflow_commit:40hex,
 source:{commit:40hex, tree:40hex, archive_sha256:64hex,
 lockfile_sha256:64hex, base_patch_sha256:64hex, receipt_sha256:64hex,
 license_files:{path:64hex}},
 workflow:{run_id:positive decimal string, run_attempt:positive decimal string,
 recipe_sha256:64hex},
 artifacts:[{target:TRIPLE, name:"TOOL-VERSION-TRIPLE.tar.gz",
 archive_sha256:64hex, binary_sha256:64hex,
 qualification:{predicate_type:PREDICATE, passed:true, abi:ABI, cases:POSITIVE_COUNT,
 report_sha256:64hex, qualified_receipt_sha256:64hex, candidate_receipt_sha256:64hex,
 sourceartifact_id:POSITIVE_INT, sourceartifact_api_digest:"sha256:64hex"}}]}.
All three TARGETS required. The custom attestation predicate is exactly
{schema:1, tool, version, source, workflow, target, binary_sha256,
 qualification:manifest artifact qualification without predicate_type}. Workflow identity is independent of tool source.
The publisher never executes archive contents. A failure leaves any new release
in draft for investigation; retry with an existing tag/release is forbidden.
"""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import struct
import subprocess
import tarfile
import tempfile

from immutable_publication import publish_snapshot, stage_evidence
from owned_tool_source import OWNED_VERSION_PATTERN, recipe_sha
from owned_tool_qualification_evidence import validate_claim

REPO = "tailrocks/velnor-new"
PREDICATE = "https://tailrocks.dev/owned-tool-qualification/v1"
TARGETS = {"x86_64-unknown-linux-gnu": ("elf", 62),
           "aarch64-unknown-linux-gnu": ("elf", 183),
           "aarch64-apple-darwin": ("macho", 0x100000c)}
LIMIT = 512 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def closed(value, keys):
    require(isinstance(value, dict) and set(value) == set(keys.split()),
            "unexpected or missing object fields")


def hex_digest(value, length):
    require(isinstance(value, str) and re.fullmatch("[0-9a-f]{%d}" % length, value),
            "invalid digest")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def parse_json(data):
    return json.loads(data, object_pairs_hook=unique_object)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def validate_manifest(manifest):
    closed(manifest, "schema tool version tag workflow_commit source workflow artifacts")
    require(type(manifest["schema"]) is int and manifest["schema"] == 1,
            "unsupported schema")
    tool, version = manifest["tool"], manifest["version"]
    require(tool in ("mise", "mbx"), "unsupported tool")
    require(isinstance(version, str) and re.fullmatch(OWNED_VERSION_PATTERN, version),
        "version must identify an owned revision")
    require(manifest["tag"] == tool + "-v" + version, "unexpected tag")
    hex_digest(manifest["workflow_commit"], 40)
    source = manifest["source"]
    closed(source, "commit tree archive_sha256 lockfile_sha256 base_patch_sha256 receipt_sha256 license_files")
    for key in ("commit", "tree"):
        hex_digest(source[key], 40)
    for key in ("archive_sha256", "lockfile_sha256", "base_patch_sha256", "receipt_sha256"):
        hex_digest(source[key], 64)
    licenses = source["license_files"]
    require(isinstance(licenses, dict) and "LICENSE" in licenses, "source license required")
    for name, value in licenses.items():
        require(isinstance(name, str) and "\\" not in name and all(
            part not in ("", ".", "..") for part in name.split("/")), "unsafe license path")
        hex_digest(value, 64)
    closed(manifest["workflow"], "run_id run_attempt recipe_sha256")
    for key in ("run_id", "run_attempt"):
        require(isinstance(manifest["workflow"][key], str) and
                re.fullmatch(r"[1-9][0-9]*", manifest["workflow"][key]), "invalid run identity")
    hex_digest(manifest["workflow"]["recipe_sha256"], 64)
    require(manifest["workflow"]["recipe_sha256"] == recipe_sha(tool),
            "manifest does not identify the current fixed build recipe")
    artifacts = manifest["artifacts"]
    require(isinstance(artifacts, list) and len(artifacts) == 3,
            "all three native artifacts required")
    found = set()
    for artifact in artifacts:
        closed(artifact, "target name archive_sha256 binary_sha256 qualification")
        target = artifact["target"]
        require(isinstance(target, str) and target in TARGETS and target not in found,
                "unexpected or duplicate native target")
        found.add(target)
        require(artifact["name"] == f"{tool}-{version}-{target}.tar.gz",
                "unexpected artifact name")
        for key in ("archive_sha256", "binary_sha256"):
            hex_digest(artifact[key], 64)
        validate_claim(artifact["qualification"], tool)



def read_regular(path):
    require(path.is_absolute() and ".." not in path.parts, "absolute safe path required")
    directory = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in path.parts[1:-1]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=directory)
            os.close(directory)
            directory = child
        descriptor = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
    finally:
        os.close(directory)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1
                and metadata.st_size <= LIMIT, "artifact must be singly linked regular file")
        data = stream.read(LIMIT + 1)
    require(len(data) <= LIMIT, "artifact size limit exceeded")
    return data


def native_binary(data, target):
    kind, machine = TARGETS[target]
    require(len(data) >= 64, "truncated native binary")
    if kind == "elf":
        require(data[:7] == b"\x7fELF\x02\x01\x01" and
                struct.unpack_from("<H", data, 18)[0] == machine and
                struct.unpack_from("<H", data, 16)[0] in (2, 3),
                "wrong ELF architecture or executable type")
    else:
        require(data[:4] == b"\xcf\xfa\xed\xfe" and
                struct.unpack_from("<I", data, 4)[0] == machine and
                struct.unpack_from("<I", data, 12)[0] == 2,
                "wrong Mach-O architecture or executable type")


def validate_archive(data, artifact, tool, source):
    require(sha(data) == artifact["archive_sha256"], "archive digest mismatch")
    expected = "mise/bin/mise" if tool == "mise" else "mbx"
    directories = {"mise", "mise/bin"} if tool == "mise" else set()
    binary = None
    license_name = "mise/LICENSE" if tool == "mise" else "LICENSE"
    license_data = None
    seen = set()
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for entry in archive:
            name = entry.name.rstrip("/") if entry.isdir() else entry.name
            require(name not in seen and "\\" not in name and
                    all(part not in ("", ".", "..") for part in name.split("/")),
                    "duplicate or unsafe archive member")
            seen.add(name)
            if entry.isdir():
                require(name in directories, "unexpected archive directory")
                continue
            require(name in (expected, license_name) and entry.isfile() and
                    entry.size <= LIMIT and not entry.mode & 0o6000,
                    "unexpected archive member or mode")
            if name == expected:
                require(entry.mode & 0o111, "binary must be executable")
            stream = archive.extractfile(entry)
            require(stream is not None, "missing binary member")
            payload = stream.read(LIMIT + 1)
            if name == expected:
                binary = payload
            else:
                license_data = payload
    require(license_data is not None and
            sha(license_data) == source["license_files"]["LICENSE"], "license digest mismatch")
    require(binary is not None and len(binary) <= LIMIT, "missing or oversized binary")
    require(sha(binary) == artifact["binary_sha256"], "binary digest mismatch")
    native_binary(binary, artifact["target"])


def gh(*arguments, cwd=None):
    environment = dict(os.environ, GH_HOST="github.com", GH_REPO=REPO)
    result = subprocess.run(["gh", *arguments], capture_output=True, check=False,
                            env=environment, cwd=cwd)
    if result.returncode:
        raise subprocess.CalledProcessError(result.returncode, result.args,
                                            result.stdout, result.stderr)
    return result.stdout


def verify_attestation(path, manifest, artifact, bundle=None):
    commit = manifest["workflow_commit"]
    options = [] if bundle is None else ["--bundle", str(bundle)]
    verified = parse_json(gh("attestation", "verify", str(path), "--repo", REPO,
        "--signer-workflow", REPO + "/.github/workflows/owned-tools.yml",
        "--signer-digest", commit, "--source-digest", commit,
        "--source-ref", "refs/heads/main", "--deny-self-hosted-runners",
        "--predicate-type", PREDICATE, "--format", "json", *options))
    expected = {"schema": 1, "tool": manifest["tool"], "version": manifest["version"],
        "source": manifest["source"], "workflow": manifest["workflow"],
        "target": artifact["target"],
        "binary_sha256": artifact["binary_sha256"], "qualification":
        {key: value for key, value in artifact["qualification"].items() if key != "predicate_type"}}
    require(isinstance(verified, list) and verified, "no verified attestations")
    for result in verified:
        statement = result["verificationResult"]["statement"]
        subjects = statement.get("subject")
        if (statement.get("_type") == "https://in-toto.io/Statement/v1" and
                statement.get("predicateType") == PREDICATE and
                json.dumps(statement.get("predicate"), sort_keys=True) ==
                json.dumps(expected, sort_keys=True) and
                statement["predicate"]["qualification"]["passed"] is True and
                subjects == [{"name": artifact["name"], "digest":
                              {"sha256": artifact["archive_sha256"]}}]):
            return
    raise ValueError("verified attestation does not match artifact/source/qualification")


def trusted_dispatch(manifest):
    require(os.environ.get("GITHUB_EVENT_NAME") == "workflow_dispatch" and
            os.environ.get("GITHUB_REF") == "refs/heads/main" and
            os.environ.get("GITHUB_REPOSITORY") == REPO and
            os.environ.get("GITHUB_SHA") == manifest["workflow_commit"] and
            os.environ.get("GITHUB_RUN_ID") == manifest["workflow"]["run_id"] and
            os.environ.get("GITHUB_RUN_ATTEMPT") == manifest["workflow"]["run_attempt"],
            "publication requires trusted main workflow dispatch")


def publish(manifest_path, artifact_directory):
    manifest = parse_json(read_regular(manifest_path.absolute()))
    validate_manifest(manifest)
    trusted_dispatch(manifest)
    with tempfile.TemporaryDirectory(prefix="owned-publish-") as temporary:
        snapshot = Path(temporary).resolve()
        for artifact in manifest["artifacts"]:
            data = read_regular(artifact_directory.absolute() / artifact["name"])
            validate_archive(data, artifact, manifest["tool"], manifest["source"])
            path = snapshot / artifact["name"]
            path.write_bytes(data)
            verify_attestation(path, manifest, artifact)
        stage_evidence(snapshot, manifest, artifact_directory.absolute(), read_regular,
                       gh, verify_attestation)
        publish_snapshot(snapshot, manifest, gh, verify_attestation, validate_archive)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--artifacts", required=True, type=Path)
    arguments = parser.parse_args()
    try:
        publish(arguments.manifest, arguments.artifacts)
    except (ValueError, OSError, KeyError, TypeError, tarfile.TarError,
            subprocess.CalledProcessError) as error:
        raise SystemExit("publication failed: " + str(error)) from error
    print("Published verified archive bytes; policy promotion requires separate review.")


if __name__ == "__main__":
    main()
