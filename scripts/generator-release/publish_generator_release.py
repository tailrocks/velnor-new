#!/usr/bin/env python3
"""Publish a source-bound Velnor generator release from uploaded API evidence."""

from __future__ import annotations

import json
import os
import re
import stat
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import NoReturn

REPOSITORY = "tailrocks/velnor-new"
WORKFLOW = ".github/workflows/generator-release.yml"
GH_VERSION = "2.102.0"
HELPER = Path(__file__).with_name("create-release-manifest.py")
TARGETS = {
    "x86_64-unknown-linux-gnu": Path("linux-assets"),
    "aarch64-apple-darwin": Path("macos-assets"),
}
MANIFEST_NAME = "velnor-actions-release-manifest.json"


def fail(problem: str) -> NoReturn:
    raise ValueError(problem)


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
        fail(failure_reason or f"command_failed:{arguments[0]}:{result.returncode}")
    return result.stdout.strip()


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


def mise_prefix() -> list[str]:
    return ["mise", "--no-config", "--no-env", "--no-hooks"]


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


def release_json(directory: Path, tag: str, name: str) -> Path:
    identifier = release_id(tag)
    value = gh_json(f"repos/{REPOSITORY}/releases/{identifier}")
    if value.get("id") != identifier or value.get("tag_name") != tag:
        fail("github_release_api_identity_mismatch")
    path = directory / name
    write_exclusive(path, (json.dumps(value, separators=(",", ":")) + "\n").encode())
    return path


def release_id(tag: str) -> int:
    """Resolve a draft through the pinned GH client, which supports draft tags."""
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
        or type(value.get("isDraft")) is not bool
    ):
        fail("github_release_view_identity_mismatch")
    return identifier


def verify_immutable_release_policy() -> None:
    """Require an enabled immutable policy before creating or publishing a release.

    GitHub requires repository Administration:read access for this check.
    Missing access is a hard failure before the source tag or draft exists.
    """
    value = gh_json(
        f"repos/{REPOSITORY}/immutable-releases",
        failure_reason="immutable_release_policy_unavailable",
    )
    if value.get("enabled") is not True:
        fail("immutable_release_policy_disabled")


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


def stage_assets(version: str, directory: Path) -> None:
    for target, source in TARGETS.items():
        binary = f"velnor-actions-{version}-{target}"
        for name in (binary, f"{binary}.sha256"):
            path = source / name
            try:
                if not stat.S_ISREG(path.lstat().st_mode):
                    fail(f"asset_not_regular:{name}")
                content = path.read_bytes()
            except OSError as error:
                fail(f"asset_read_failed:{name}:{error}")
            if not content:
                fail(f"asset_empty:{name}")
            write_exclusive(directory / name, content)


def manifest_command(
    mode: str, release: Path, directory: Path, manifest: Path, version: str,
    commit: str, tag: str, resolved_commit: str,
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


def verify_event(commit: str) -> None:
    required = {
        "GITHUB_REPOSITORY": REPOSITORY,
        "GITHUB_REF": "refs/heads/main",
        "GITHUB_EVENT_NAME": "workflow_dispatch",
        "GITHUB_WORKFLOW_REF": f"{REPOSITORY}/{WORKFLOW}@refs/heads/main",
        "GITHUB_WORKFLOW_SHA": commit,
    }
    if any(os.environ.get(key) != value for key, value in required.items()):
        fail("release_requires_exact_main_workflow_dispatch")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        fail("source_commit_invalid")
    if not os.environ.get("GH_TOKEN"):
        fail("release_token_missing")


def verify_source(commit: str) -> None:
    if run_command(["git", "rev-parse", "HEAD"]) != commit or current_main() != commit:
        fail("release_source_not_current_main")


def publish(version: str) -> None:
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        fail("malformed_version")
    commit = os.environ.get("GITHUB_SHA", "")
    verify_event(commit)
    mise([], install=True)
    verify_source(commit)
    verify_immutable_release_policy()
    tag = f"generator-{commit}"
    runner_temp = Path(os.environ.get("RUNNER_TEMP", ""))
    if not runner_temp.is_absolute() or not runner_temp.is_dir():
        fail("runner_temp_unavailable")
    with tempfile.TemporaryDirectory(
        prefix="velnor-generator-release-", dir=runner_temp
    ) as scratch:
        directory = Path(scratch)
        stage_assets(version, directory)
        create_source_tag(tag, commit)
        mise([
            "release", "create", tag, "--repo", REPOSITORY,
            "--target", commit, "--title", tag, "--latest=false", "--draft",
            "--notes", f"velnor-actions {version} built from {commit}.",
        ])
        files = []
        for target in TARGETS:
            binary = f"velnor-actions-{version}-{target}"
            files.extend((str(directory / binary), str(directory / f"{binary}.sha256")))
        mise(["release", "upload", tag, *files, "--repo", REPOSITORY])
        manifest = directory / MANIFEST_NAME
        first = release_json(directory, tag, "draft-assets.json")
        source_tag = tag_commit(tag)
        manifest_command("create", first, directory, manifest, version, commit, tag, source_tag)
        mise(["release", "upload", tag, str(manifest), "--repo", REPOSITORY])

        draft = release_json(directory, tag, "draft-manifest.json")
        source_tag = tag_commit(tag)
        manifest_command(
            "verify-draft", draft, directory, manifest, version, commit, tag, source_tag,
        )
        verify_immutable_release_policy()
        if current_main() != commit:
            fail("source_commit_no_longer_current_main")
        mise(["release", "edit", tag, "--repo", REPOSITORY, "--draft=false"])

        published = release_json(directory, tag, "published.json")
        source_tag = tag_commit(tag)
        manifest_command(
            "verify", published, directory, manifest, version, commit, tag, source_tag,
        )
        if current_main() != commit:
            fail("source_commit_no_longer_current_main")


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    try:
        publish(args.version)
        return 0
    except (OSError, ValueError, UnicodeError) as error:
        print(f"generator release publication: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
