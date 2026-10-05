"""Pinned GitHub CLI operations and verified release metadata output."""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import NoReturn

REPOSITORY = "tailrocks/velnor-new"
GH_VERSION = "2.102.0"
HELPER = Path(__file__).with_name("create-release-manifest.py")
TARGETS = ("x86_64-unknown-linux-gnu", "aarch64-apple-darwin")
MANIFEST_NAME = "velnor-actions-release-manifest.json"
MANIFEST_CHECKSUM_NAME = f"{MANIFEST_NAME}.sha256"
ACCEPTED_DIRECTORY_NAME = "velnor-generator-accepted"
ACCEPTANCE_NAME = "velnor-actions-release-acceptance.json"


def fail(problem: str) -> NoReturn:
    raise ValueError(problem)


def valid_release_version(version: str) -> bool:
    """Match the contract crate's exact stable X.Y.Z release grammar."""
    parts = version.split(".")
    return len(parts) == 3 and all(
        re.fullmatch(r"0|[1-9][0-9]*", part) is not None
        and (
            len(part) < 20
            or (len(part) == 20 and part <= "18446744073709551615")
        )
        for part in parts
    )


def run_command(
    arguments: list[str],
    install: bool = False,
    failure_reason: str | None = None,
    clear_tokens: bool = False,
) -> str:
    environment = os.environ.copy()
    if install or clear_tokens:
        for name in ("GH_TOKEN", "GITHUB_TOKEN", "MISE_GITHUB_TOKEN"):
            if install:
                environment[name] = ""
            else:
                environment.pop(name, None)
    try:
        result = subprocess.run(
            arguments, capture_output=True, text=True, check=False, env=environment
        )
    except OSError as error:
        fail(f"command_start_failed:{arguments[0]}:{error}")
    if result.returncode != 0:
        if failure_reason == "manifest_validation_failed":
            line = result.stderr.strip().splitlines()
            prefix = "generator release manifest: "
            if line and line[0].startswith(prefix):
                fail(f"{failure_reason}:{line[0][len(prefix):][:200]}")
        fail(failure_reason or f"command_failed:{arguments[0]}:{result.returncode}")
    return result.stdout.strip()


def mise_prefix() -> list[str]:
    return ["mise", "--no-config", "--no-env", "--no-hooks"]


def mise(
    arguments: list[str],
    install: bool = False,
    failure_reason: str | None = None,
) -> str:
    prefix = mise_prefix()
    if install:
        return run_command([*prefix, "install", f"gh@{GH_VERSION}"], install=True)
    return run_command(
        [*prefix, "exec", f"gh@{GH_VERSION}", "--", "gh", *arguments],
        failure_reason=failure_reason,
    )


def gh_json(endpoint: str, failure_reason: str | None = None) -> dict[str, object]:
    try:
        value = json.loads(mise(["api", endpoint], failure_reason=failure_reason))
    except json.JSONDecodeError as error:
        fail(f"github_api_json_invalid:{error}")
    if not isinstance(value, dict):
        fail("github_api_object_missing")
    return value


def read_json(path: Path) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        fail(f"release_json_invalid:{path.name}:{error}")
    if not isinstance(value, dict):
        fail(f"release_json_not_object:{path.name}")
    return value


def current_main() -> str:
    value = gh_json(f"repos/{REPOSITORY}/commits/main").get("sha")
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value):
        fail("current_main_commit_invalid")
    return value


def tag_commit(tag: str) -> str:
    reference = gh_json(f"repos/{REPOSITORY}/git/ref/tags/{tag}")
    if reference.get("ref") != f"refs/tags/{tag}":
        fail("release_tag_reference_mismatch")
    tagged = reference.get("object")
    seen: set[str] = set()
    for _ in range(8):
        if not isinstance(tagged, dict):
            fail("release_tag_object_missing")
        kind, digest = tagged.get("type"), tagged.get("sha")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{40}", digest):
            fail("release_tag_object_sha_invalid")
        if kind == "commit":
            return digest
        if kind != "tag" or digest in seen:
            fail("release_tag_does_not_resolve_to_commit")
        seen.add(digest)
        tagged = gh_json(f"repos/{REPOSITORY}/git/tags/{digest}").get("object")
    fail("release_tag_annotation_depth_exceeded")


def write_exclusive(path: Path, value: bytes) -> None:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(value)
    except OSError:
        path.unlink(missing_ok=True)
        raise


def release_json(
    directory: Path, identifier: int, tag: str, name: str, draft: bool
) -> Path:
    value = gh_json(f"repos/{REPOSITORY}/releases/{identifier}")
    if (
        value.get("id") != identifier
        or value.get("tag_name") != tag
        or value.get("draft") is not draft
        or value.get("url")
        != f"https://api.github.com/repos/{REPOSITORY}/releases/{identifier}"
        or value.get("html_url")
        != f"https://github.com/{REPOSITORY}/releases/tag/{tag}"
    ):
        fail("github_release_api_identity_mismatch")
    path = directory / name
    write_exclusive(path, (json.dumps(value, separators=(",", ":")) + "\n").encode())
    return path


def release_id(tag: str) -> int:
    """Resolve the draft once; bind every following API read to its release ID."""
    try:
        value = json.loads(
            mise([
                "release", "view", tag, "--repo", REPOSITORY,
                "--json", "databaseId,tagName,isDraft",
            ])
        )
    except json.JSONDecodeError as error:
        fail(f"github_release_view_json_invalid:{error}")
    if not isinstance(value, dict):
        fail("github_release_view_invalid")
    identifier = value.get("databaseId")
    if (
        type(identifier) is not int
        or identifier <= 0
        or value.get("tagName") != tag
        or value.get("isDraft") is not True
    ):
        fail("github_release_view_identity_mismatch")
    return identifier


def create_source_tag(tag: str, commit: str) -> None:
    """Create and verify a lightweight source tag before making a draft release."""
    try:
        value = json.loads(
            mise([
                "api", f"repos/{REPOSITORY}/git/refs", "--method", "POST",
                "-f", f"ref=refs/tags/{tag}", "-f", f"sha={commit}",
            ])
        )
    except json.JSONDecodeError as error:
        fail(f"github_tag_create_json_invalid:{error}")
    if not isinstance(value, dict) or value.get("ref") != f"refs/tags/{tag}":
        fail("github_tag_create_reference_mismatch")
    created = value.get("object")
    if (
        not isinstance(created, dict)
        or created.get("type") != "commit"
        or created.get("sha") != commit
        or tag_commit(tag) != commit
    ):
        fail("github_tag_create_source_mismatch")


def manifest_command(
    mode: str,
    release: Path,
    directory: Path,
    manifest: Path,
    version: str,
    commit: str,
    tag: str,
    resolved_commit: str,
) -> str:
    return run_command(
        [
            sys.executable,
            str(HELPER),
            "--mode",
            mode,
            "--release-json",
            str(release),
            "--asset-dir",
            str(directory),
            "--manifest",
            str(manifest),
            "--version",
            version,
            "--repository",
            REPOSITORY,
            "--commit",
            commit,
            "--tag",
            tag,
            "--tag-commit",
            resolved_commit,
        ],
        failure_reason="manifest_validation_failed",
        clear_tokens=True,
    )


def accepted_metadata(
    output: Path,
    assets: Path,
    release: dict[str, object],
    version: str,
    commit: str,
    tag: str,
    identifier: int,
) -> None:
    """Emit pin metadata only after the published release passed every check."""
    if output.exists() or output.is_symlink():
        fail("accepted_metadata_path_exists")
    if (
        release.get("id") != identifier
        or release.get("tag_name") != tag
        or release.get("draft") is not False
        or release.get("immutable") is not True
    ):
        fail("accepted_release_identity_unverified")
    records = release.get("assets")
    if not isinstance(records, list):
        fail("accepted_release_assets_missing")
    if any(
        not isinstance(record, dict) or not isinstance(record.get("name"), str)
        for record in records
    ):
        fail("accepted_release_asset_invalid")
    expected_names = []
    for target in TARGETS:
        binary = f"velnor-actions-{version}-{target}"
        expected_names.extend((binary, f"{binary}.sha256"))
    expected_names.extend((MANIFEST_NAME, MANIFEST_CHECKSUM_NAME))
    if {record["name"] for record in records} != set(expected_names):
        fail("accepted_release_asset_set_mismatch")
    receipt_assets = []
    for record in sorted(records, key=lambda item: item["name"]):
        if (
            not isinstance(record.get("browser_download_url"), str)
            or not isinstance(record.get("digest"), str)
            or type(record.get("size")) is not int
        ):
            fail("accepted_release_asset_fields_invalid")
        receipt_assets.append(
            {
                "name": record["name"],
                "browser_download_url": record["browser_download_url"],
                "digest": record["digest"],
                "size": record["size"],
            }
        )
    receipt = {
        "schema": 1,
        "repository": REPOSITORY,
        "version": version,
        "source_commit": commit,
        "tag": tag,
        "release_id": identifier,
        "immutable": True,
        "assets": receipt_assets,
    }
    value = (json.dumps(receipt, separators=(",", ":"), sort_keys=True) + "\n").encode()
    with tempfile.TemporaryDirectory(
        prefix="velnor-generator-accepted-", dir=output.parent
    ) as staging_name:
        staging = Path(staging_name)
        for name in (MANIFEST_NAME, MANIFEST_CHECKSUM_NAME):
            write_exclusive(staging / name, (assets / name).read_bytes())
        write_exclusive(staging / ACCEPTANCE_NAME, value)
        os.replace(staging, output)
