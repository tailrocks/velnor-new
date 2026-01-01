#!/usr/bin/env python3
"""Plan Debian development versions and link a no-build package to its inputs.

This evidence helper is not part of the Velnor runtime and is not installed.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import subprocess
import sys
import tarfile
from pathlib import Path
from typing import Any

from debian_package_common import (
    COMMIT_RE,
    archive_member_sha256,
    DEBIAN_UPSTREAM_RE,
    DEV_VERSION_RE,
    PACKAGE_NAME,
    SHA256_RE,
    TARGETS,
    TOOL_VERSION,
    TREE_RE,
    EvidenceError,
    load_json,
    require_sha,
    sha256_file,
)
from debian_package_artifact import (
    read_deb_identity,
    validate_build_provenance,
    verify_package,
)


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, path)

def normalize_utc(value: str) -> tuple[str, str]:
    try:
        parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise EvidenceError("commit time must be ISO-8601 with an explicit UTC offset") from exc
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        raise EvidenceError("commit time must include an explicit UTC offset")
    normalized = parsed.astimezone(dt.timezone.utc)
    return normalized.strftime("%Y-%m-%dT%H:%M:%SZ"), normalized.strftime("%Y%m%d%H%M%S")

def dpkg_compare(left: str, operator: str, right: str) -> bool:
    try:
        result = subprocess.run(
            ["dpkg", "--compare-versions", left, operator, right],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
    except FileNotFoundError as exc:
        raise EvidenceError("dpkg is required to validate Debian version ordering") from exc
    if result.returncode == 0:
        return True
    if result.returncode == 1:
        return False
    detail = result.stderr.strip()
    raise EvidenceError(f"dpkg rejected a version string: {detail or left + ' ' + operator + ' ' + right}")

def compute_version(
    upstream: str, sequence: int, commit: str, commit_time: str, previous: str
) -> tuple[str, str]:
    if not DEBIAN_UPSTREAM_RE.fullmatch(upstream) or "-" in upstream:
        raise EvidenceError("next upstream version must be a Debian-safe numeric release version")
    if not 1 <= sequence <= 9999:
        raise EvidenceError("qualification sequence must be between 1 and 9999")
    if not COMMIT_RE.fullmatch(commit):
        raise EvidenceError("source commit must be a full lowercase Git SHA-1")
    utc_text, compact_utc = normalize_utc(commit_time)
    previous_match = DEV_VERSION_RE.fullmatch(previous)
    if previous_match:
        previous_sequence = int(previous_match.group("sequence"))
        if previous_match.group("upstream") != upstream or sequence != previous_sequence + 1:
            raise EvidenceError("development qualification sequence must increment exactly from the previous candidate")
        if previous_match.group("commit") == commit[:12]:
            raise EvidenceError("a new qualification sequence must use a distinct source commit")
    elif sequence != 1:
        raise EvidenceError("the first development candidate must use sequence 0001")
    candidate = f"{upstream}~dev{sequence:04d}+{compact_utc}+g{commit[:12]}-1"
    release = f"{upstream}-1"
    if not dpkg_compare(previous, "lt", candidate):
        raise EvidenceError(f"candidate version {candidate} is not newer than previous {previous}")
    if not dpkg_compare(candidate, "lt", release):
        raise EvidenceError(f"candidate version {candidate} must sort before release {release}")
    return candidate, utc_text

def validate_tool_record(path: Path) -> dict[str, Any]:
    record = load_json(path)
    upstream = record.get("upstream", {})
    private_install = record.get("private_install", {})
    if upstream.get("latest_stable_tag") != f"v{TOOL_VERSION}":
        raise EvidenceError("cargo-deb acquisition record does not pin the expected release")
    source = upstream.get("source_checkout", {})
    if source.get("head") != upstream.get("tag_commit"):
        raise EvidenceError("cargo-deb source tag and checkout commit differ")
    tool_root = path.parent
    tool_path = tool_root / str(private_install.get("extraction_path", ""))
    tool_hash = require_sha(private_install.get("extracted_binary_sha256"), "cargo-deb binary SHA-256")
    if not tool_path.is_file() or sha256_file(tool_path) != tool_hash:
        raise EvidenceError("private cargo-deb binary does not match its acquisition record")
    version = subprocess.run(
        [str(tool_path), "--version"], capture_output=True, text=True, check=False
    )
    if version.returncode != 0 or version.stdout.strip() != f"cargo-deb {TOOL_VERSION}":
        raise EvidenceError("private cargo-deb binary did not report its pinned version")
    help_path = tool_root / "help.txt"
    help_artifact = record.get("artifacts", {}).get("help.txt", {})
    help_hash = require_sha(help_artifact.get("sha256"), "cargo-deb help SHA-256")
    if not help_path.is_file() or sha256_file(help_path) != help_hash:
        raise EvidenceError("cargo-deb help snapshot does not match its acquisition record")
    help_text = help_path.read_text(encoding="utf-8")
    for flag in ("--no-build", "--deb-version <version>"):
        if flag not in help_text:
            raise EvidenceError(f"pinned cargo-deb help does not document {flag}")
    return {
        "version": TOOL_VERSION,
        "source_repository": upstream.get("repository"),
        "source_tag": upstream.get("latest_stable_tag"),
        "source_commit": source.get("head"),
        "source_archive_sha256": source.get("git_archive_sha256"),
        "release_asset_sha256": upstream.get("official_release_asset", {}).get("downloaded_sha256"),
        "binary_path": str(tool_path),
        "binary_sha256": tool_hash,
        "help_snapshot_sha256": help_hash,
        "source_refs": {
            "README.md": "README.md:223,235",
            "src/lib.rs": "src/lib.rs:120 (cargo_build is skipped when no_build=true)",
            "src/main.rs": "src/main.rs:53,60 (full version override and no-build options)",
            "src/config.rs": "src/config.rs:995-1001,1257 (version override is validated and written to control)",
        },
    }

def load_source_receipt(path: Path) -> dict[str, Any]:
    receipt = load_json(path)
    commit = receipt.get("commit")
    tree = receipt.get("tree")
    if not isinstance(commit, str) or not COMMIT_RE.fullmatch(commit):
        raise EvidenceError("source receipt must contain a full lowercase commit SHA")
    if not isinstance(tree, str) or not TREE_RE.fullmatch(tree):
        raise EvidenceError("source receipt must contain a full lowercase tree SHA")
    if receipt.get("state") != "published" or receipt.get("remote_after") != commit:
        raise EvidenceError("source receipt does not prove that its exact commit was published")
    archive_path = Path(str(receipt.get("archive_path", "")))
    archive_hash = require_sha(receipt.get("archive_sha256"), "source archive SHA-256")
    if not archive_path.is_file() or sha256_file(archive_path) != archive_hash:
        raise EvidenceError("source archive does not match the exact publication receipt")
    return {
        "repository": "tailrocks/velnor-new",
        "pr": 97,
        "branch": receipt.get("branch"),
        "commit": commit,
        "tree": tree,
        "archive_path": str(archive_path),
        "archive_sha256": archive_hash,
        "receipt_path": str(path),
        "receipt_sha256": sha256_file(path),
    }

def make_plan(args: argparse.Namespace) -> dict[str, Any]:
    source = load_source_receipt(args.source_receipt)
    if args.commit != source["commit"] or args.tree != source["tree"]:
        raise EvidenceError("explicit commit/tree do not match the publication receipt")
    version, utc = compute_version(
        args.next_upstream_version,
        args.sequence,
        source["commit"],
        args.committer_time,
        args.previous_version,
    )
    if args.architecture not in TARGETS:
        raise EvidenceError(f"unsupported Debian architecture for the qualified target map: {args.architecture}")
    target, _machine = TARGETS[args.architecture]
    tool = validate_tool_record(args.cargo_deb_record)
    lockfile_hash = archive_member_sha256(Path(source["archive_path"]), "Cargo.lock")
    return {
        "evidence_id": f"PKG-DEVELOPMENT-VERSION-{source['commit'][:12]}-Q{args.sequence:04d}",
        "status": "VERSION_PLAN_ONLY_NO_BINARY_OR_DEB",
        "repository": source["repository"],
        "pr": source["pr"],
        "branch": source["branch"],
        "source": {
            "commit": source["commit"],
            "tree": source["tree"],
            "committer_time_input": args.committer_time,
            "committer_time_utc": utc,
            "archive_path": source["archive_path"],
            "archive_sha256": source["archive_sha256"],
            "cargo_lock_sha256": lockfile_hash,
            "publication_receipt_path": source["receipt_path"],
            "publication_receipt_sha256": source["receipt_sha256"],
        },
        "package": {
            "name": PACKAGE_NAME,
            "cargo_upstream_version_at_source": "0.1.1",
            "next_release_upstream_version": args.next_upstream_version,
            "version": version,
            "previous_version_for_order_check": args.previous_version,
            "sequence": args.sequence,
            "architecture": args.architecture,
            "rust_target": target,
            "artifact_name": f"{PACKAGE_NAME}_{version}_{args.architecture}.deb",
        },
        "cargo_deb": tool,
        "packaging_contract": {
            "command_template": [
                "<verified-cargo-deb-3.8.0>",
                "--manifest-path",
                "<exact-extracted-source>/crates/tools/velnor-runner-cli/Cargo.toml",
                "--target",
                target,
                "--no-build",
                "--no-strip",
                "--deb-version",
                version,
                "--output",
                "<new-empty-output-directory>",
            ],
            "verified_binary_sha256_required_before_package_command": True,
            "package_record_must_link_source_commit_tree_archive_binary_and_deb_sha256": True,
            "install_or_service_action": False,
        },
        "version_policy": {
            "scheme": "<next-upstream>~dev<4-digit-qualification-sequence>+<UTC-commit-time>+g<12-char-commit>-1",
            "ordering": "sequence orders qual A before qual B even if commit times or hashes do not sort",
            "architecture_identity": "Debian control Architecture and artifact filename; shared version across architectures",
            "old_schema_rollback": "historical v7 0.1.1-1 is not a rollback target for a newer journal schema",
        },
        "limitations": [
            "This is a development version plan only; no current-source binary or .deb is supplied or claimed.",
            "The 0.1.2 upstream value is a candidate development line; final product versioning remains subject to project release approval.",
            "A and B must use distinct, meaningful, verified binaries and consecutive qualification sequence values on one supported journal schema.",
            "Version ordering alone does not prove schema or binary rollback compatibility.",
            "No install, service, release, or publication action is performed by this helper.",
        ],
    }

def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="command", required=True)
    plan = commands.add_parser("plan", help="verify a published source receipt and mint an ordered Debian version")
    plan.add_argument("--source-receipt", type=Path, required=True)
    plan.add_argument("--commit", required=True)
    plan.add_argument("--tree", required=True)
    plan.add_argument("--committer-time", required=True, help="ISO-8601 commit time with an explicit UTC offset")
    plan.add_argument("--architecture", required=True, choices=sorted(TARGETS))
    plan.add_argument("--next-upstream-version", required=True)
    plan.add_argument("--sequence", type=int, required=True)
    plan.add_argument("--previous-version", required=True)
    plan.add_argument("--cargo-deb-record", type=Path, required=True)
    plan.add_argument("--output", type=Path, required=True)
    verify = commands.add_parser("verify-package", help="link a plan, build record, binary, and no-build .deb")
    verify.add_argument("--plan", type=Path, required=True)
    verify.add_argument("--binary-build-record", type=Path, required=True)
    verify.add_argument("--deb", type=Path, required=True)
    verify.add_argument("--output", type=Path, required=True)
    return root

def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "plan":
            result = make_plan(args)
        else:
            result = verify_package(args)
        write_json(args.output, result)
    except EvidenceError as exc:
        print(f"package-development: {exc}", file=sys.stderr)
        return 2
    except (OSError, KeyError, TypeError, ValueError) as exc:
        print(f"package-development: invalid evidence: {exc}", file=sys.stderr)
        return 2
    print(json.dumps({"status": result["status"], "evidence_id": result["evidence_id"], "output": str(args.output)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
