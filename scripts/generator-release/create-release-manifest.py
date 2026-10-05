#!/usr/bin/env python3
"""Create or verify a generator manifest from GitHub's uploaded asset records."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import sys
from pathlib import Path
from typing import NoReturn
from urllib.parse import urlsplit

from github_release_api import TARGETS, valid_release_version

REPOSITORY = "tailrocks/velnor-new"
MANIFEST_NAME = "velnor-actions-release-manifest.json"
MANIFEST_CHECKSUM_NAME = f"{MANIFEST_NAME}.sha256"


def fail(problem: str) -> NoReturn:
    raise ValueError(problem)


def read_json(path: Path) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        fail(f"invalid_json:{path.name}:{error}")
    if not isinstance(value, dict):
        fail(f"invalid_object:{path.name}")
    return value


def read_regular(path: Path) -> bytes:
    try:
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode):
            fail(f"asset_not_regular:{path.name}")
        value = path.read_bytes()
    except OSError as error:
        fail(f"asset_read_failed:{path.name}:{error}")
    if not value:
        fail(f"asset_empty:{path.name}")
    return value


def sha256(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def asset_names(version: str) -> tuple[str, ...]:
    names: list[str] = []
    for target, _source_directory in TARGETS:
        binary = f"velnor-actions-{version}-{target}"
        names.extend((binary, f"{binary}.sha256"))
    return tuple(names)


def release_asset_names(version: str, mode: str) -> set[str]:
    expected = set(asset_names(version))
    if mode in ("verify-draft", "verify"):
        expected.update((MANIFEST_NAME, MANIFEST_CHECKSUM_NAME))
    return expected


def valid_url(value: object, tag: str, asset: str) -> str:
    if not isinstance(value, str) or not value or not value.isprintable():
        fail(f"missing_browser_download_url:{asset}")
    if not value.startswith("https://") or any(character.isspace() for character in value):
        fail(f"noncanonical_browser_download_url:{asset}")
    parsed = urlsplit(value)
    if (
        parsed.scheme != "https"
        or parsed.netloc != "github.com"
        or parsed.query
        or parsed.fragment
        or parsed.username
        or parsed.password
    ):
        fail(f"unsafe_browser_download_url:{asset}")
    if parsed.path.split("/") != [
        "",
        "tailrocks",
        "velnor-new",
        "releases",
        "download",
        tag,
        asset,
    ]:
        fail(f"unbound_browser_download_url:{asset}")
    return value


def validate_identity(
    release: dict[str, object],
    version: str,
    repository: str,
    commit: str,
    tag: str,
    tag_commit: str,
    mode: str,
) -> None:
    if repository != REPOSITORY:
        fail("unexpected_repository")
    if not valid_release_version(version):
        fail("malformed_version")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        fail("malformed_source_commit")
    if tag != f"generator-{commit}" or tag_commit != commit:
        fail("source_tag_binding_mismatch")
    if release.get("tag_name") != tag or release.get("prerelease") is not False:
        fail("release_identity_mismatch")
    if mode in ("create", "verify-draft") and release.get("draft") is not True:
        fail("candidate_release_not_draft")
    if mode == "verify" and (
        release.get("draft") is not False or release.get("immutable") is not True
    ):
        fail("published_release_not_immutable")


def release_assets(release: dict[str, object]) -> dict[str, dict[str, object]]:
    values = release.get("assets")
    if not isinstance(values, list):
        fail("release_assets_missing")
    indexed: dict[str, dict[str, object]] = {}
    for value in values:
        if not isinstance(value, dict):
            fail("release_asset_invalid")
        name = value.get("name")
        if not isinstance(name, str) or name in indexed:
            fail("release_asset_name_invalid")
        indexed[name] = value
    return indexed


def validate_assets(
    asset_dir: Path,
    indexed: dict[str, dict[str, object]],
    version: str,
    tag: str,
    mode: str,
    manifest_path: Path,
    manifest_checksum_path: Path,
) -> dict[str, str]:
    expected_names = release_asset_names(version, mode)
    if set(indexed) != expected_names:
        fail(
            "release_asset_set_mismatch:"
            f"missing={sorted(expected_names - set(indexed))}:"
            f"unexpected={sorted(set(indexed) - expected_names)}"
        )

    if any(
        path.parent.resolve() != asset_dir.resolve()
        for path in (manifest_path, manifest_checksum_path)
    ):
        fail("manifest_path_outside_asset_directory")

    digests: dict[str, str] = {}
    for name in asset_names(version):
        local = read_regular(asset_dir / name)
        digest = sha256(local)
        record = indexed[name]
        if record.get("state") != "uploaded" or record.get("size") != len(local):
            fail(f"release_asset_state_or_size_mismatch:{name}")
        if record.get("digest") != f"sha256:{digest}":
            fail(f"release_asset_digest_mismatch:{name}")
        valid_url(record.get("browser_download_url"), tag, name)
        digests[name] = digest

    for target, _source_directory in TARGETS:
        name = f"velnor-actions-{version}-{target}"
        sidecar = read_regular(asset_dir / f"{name}.sha256")
        if sidecar != f"{digests[name]}  {name}\n".encode("ascii"):
            fail(f"release_sidecar_mismatch:{name}")

    if mode in ("verify-draft", "verify"):
        manifest = read_regular(manifest_path)
        manifest_digest = sha256(manifest)
        manifest_sidecar = read_regular(manifest_checksum_path)
        if manifest_sidecar != checksum_bytes(manifest_digest, MANIFEST_NAME):
            fail("release_manifest_sidecar_mismatch")
        for name, local in (
            (MANIFEST_NAME, manifest),
            (MANIFEST_CHECKSUM_NAME, manifest_sidecar),
        ):
            record = indexed[name]
            digest = sha256(local)
            if record.get("state") != "uploaded" or record.get("size") != len(local):
                fail(f"release_asset_state_or_size_mismatch:{name}")
            if record.get("digest") != f"sha256:{digest}":
                fail(f"release_asset_digest_mismatch:{name}")
            valid_url(record.get("browser_download_url"), tag, name)
            digests[name] = digest
    return digests


def manifest_bytes(
    indexed: dict[str, dict[str, object]],
    version: str,
    repository: str,
    commit: str,
    tag: str,
    digests: dict[str, str],
) -> bytes:
    targets = []
    for target, _source_directory in TARGETS:
        name = f"velnor-actions-{version}-{target}"
        targets.append(
            {
                "target": target,
                "artifact": valid_url(indexed[name].get("browser_download_url"), tag, name),
                "sha256": digests[name],
            }
        )
    value = {
        "schema": 1,
        "version": version,
        "repository": repository,
        "commit": commit,
        "targets": targets,
    }
    return (json.dumps(value, separators=(",", ":"), sort_keys=True) + "\n").encode("utf-8")


def checksum_bytes(digest: str, name: str) -> bytes:
    return f"{digest}  {name}\n".encode("ascii")


def write_exclusive(path: Path, value: bytes) -> None:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(value)
    except OSError:
        path.unlink(missing_ok=True)
        raise


def validate_release(
    release: dict[str, object],
    version: str,
    repository: str,
    commit: str,
    tag: str,
    tag_commit: str,
    mode: str,
    asset_dir: Path,
    manifest_path: Path,
    manifest_checksum_path: Path,
) -> bytes:
    validate_identity(release, version, repository, commit, tag, tag_commit, mode)
    indexed = release_assets(release)
    digests = validate_assets(
        asset_dir,
        indexed,
        version,
        tag,
        mode,
        manifest_path,
        manifest_checksum_path,
    )
    return manifest_bytes(indexed, version, repository, commit, tag, digests)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--mode", choices=("create", "verify-draft", "verify"), required=True
    )
    parser.add_argument("--release-json", type=Path)
    parser.add_argument("--asset-dir", type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--repository")
    parser.add_argument("--commit")
    parser.add_argument("--tag")
    parser.add_argument("--tag-commit")
    args = parser.parse_args()

    try:
        required = (
            args.release_json,
            args.asset_dir,
            args.manifest,
            args.manifest.with_name(MANIFEST_CHECKSUM_NAME)
            if args.manifest is not None
            else None,
            args.repository,
            args.commit,
            args.tag,
            args.tag_commit,
        )
        if any(value is None for value in required):
            fail("fixture_mode_arguments_missing")
        release = read_json(args.release_json)
        expected = validate_release(
            release,
            args.version,
            args.repository,
            args.commit,
            args.tag,
            args.tag_commit,
            args.mode,
            args.asset_dir,
            args.manifest,
            args.manifest.with_name(MANIFEST_CHECKSUM_NAME),
        )
        if args.mode == "create":
            write_exclusive(args.manifest, expected)
            write_exclusive(
                args.manifest.with_name(MANIFEST_CHECKSUM_NAME),
                checksum_bytes(sha256(expected), MANIFEST_NAME),
            )
        elif read_regular(args.manifest) != expected:
            fail("manifest_does_not_match_published_assets")
        elif read_regular(args.manifest.with_name(MANIFEST_CHECKSUM_NAME)) != checksum_bytes(
            sha256(expected), MANIFEST_NAME
        ):
            fail("manifest_checksum_does_not_match_published_assets")
        print(f"release_manifest_sha256={sha256(expected)}")
        print(
            "release_manifest_checksum_sha256="
            f"{sha256(checksum_bytes(sha256(expected), MANIFEST_NAME))}"
        )
        return 0
    except (OSError, ValueError, UnicodeError) as error:
        print(f"generator release manifest: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
