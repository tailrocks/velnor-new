#!/usr/bin/env python3
"""Publish a source-bound Velnor generator release from uploaded API evidence."""

from __future__ import annotations

import os
import re
import stat
import sys
import tempfile
from pathlib import Path

from github_release_api import (
    ACCEPTED_DIRECTORY_NAME,
    MANIFEST_CHECKSUM_NAME,
    MANIFEST_NAME,
    REPOSITORY,
    TARGETS,
    accepted_metadata,
    create_source_tag,
    current_main,
    fail,
    manifest_command,
    mise,
    read_json,
    release_id,
    release_json,
    run_command,
    tag_commit,
    valid_release_version,
    write_exclusive,
)

WORKFLOW = ".github/workflows/generator-release.yml"


def stage_assets(version: str, directory: Path) -> None:
    for target, source_directory in TARGETS:
        binary = f"velnor-actions-{version}-{target}"
        for name in (binary, f"{binary}.sha256"):
            path = Path(source_directory) / name
            try:
                if not stat.S_ISREG(path.lstat().st_mode):
                    fail(f"asset_not_regular:{name}")
                content = path.read_bytes()
            except OSError as error:
                fail(f"asset_read_failed:{name}:{error}")
            if not content:
                fail(f"asset_empty:{name}")
            write_exclusive(directory / name, content)


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
    if not valid_release_version(version):
        fail("malformed_version")
    commit = os.environ.get("GITHUB_SHA", "")
    verify_event(commit)
    mise([], install=True)
    verify_source(commit)
    tag = f"generator-{commit}"
    runner_temp = Path(os.environ.get("RUNNER_TEMP", ""))
    if not runner_temp.is_absolute() or not runner_temp.is_dir():
        fail("runner_temp_unavailable")
    accepted_dir = runner_temp / ACCEPTED_DIRECTORY_NAME
    if accepted_dir.exists() or accepted_dir.is_symlink():
        fail("accepted_metadata_path_exists")
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
        identifier = release_id(tag)
        files = []
        for target, _source_directory in TARGETS:
            binary = f"velnor-actions-{version}-{target}"
            files.extend((str(directory / binary), str(directory / f"{binary}.sha256")))
        mise(["release", "upload", tag, *files, "--repo", REPOSITORY])
        manifest = directory / MANIFEST_NAME
        manifest_checksum = directory / MANIFEST_CHECKSUM_NAME
        first = release_json(directory, identifier, tag, "draft-assets.json", True)
        source_tag = tag_commit(tag)
        manifest_command(
            "create", first, directory, manifest, version, commit, tag, source_tag
        )
        mise([
            "release", "upload", tag, str(manifest), str(manifest_checksum),
            "--repo", REPOSITORY,
        ])

        draft = release_json(directory, identifier, tag, "draft-manifest.json", True)
        source_tag = tag_commit(tag)
        manifest_command(
            "verify-draft", draft, directory, manifest, version, commit, tag, source_tag,
        )
        if current_main() != commit:
            fail("source_commit_no_longer_current_main")
        mise(["release", "edit", tag, "--repo", REPOSITORY, "--draft=false"])

        published_path = release_json(
            directory, identifier, tag, "published.json", False
        )
        source_tag = tag_commit(tag)
        manifest_command(
            "verify", published_path, directory, manifest, version, commit, tag,
            source_tag,
        )
        if current_main() != commit:
            fail("source_commit_no_longer_current_main")
        published = read_json(published_path)
        accepted_metadata(
            accepted_dir,
            directory,
            published,
            version,
            commit,
            tag,
            identifier,
        )


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
