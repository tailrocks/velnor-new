#!/usr/bin/env python3
"""Plan Debian development versions and link a no-build package to its inputs.

This evidence helper is not part of the Velnor runtime and is not installed.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import io
import json
import os
import posixpath
import re
import shlex
import subprocess
import sys
import tarfile
from pathlib import Path
from typing import Any


PACKAGE_NAME = "velnor-host"
TOOL_VERSION = "3.8.0"
TARGETS = {
    "amd64": ("x86_64-unknown-linux-gnu", "Advanced Micro Devices X86-64"),
    "arm64": ("aarch64-unknown-linux-gnu", "AArch64"),
    "armhf": ("armv7-unknown-linux-gnueabihf", "ARM"),
    "i386": ("i686-unknown-linux-gnu", "Intel 80386"),
}
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
TREE_RE = re.compile(r"^[0-9a-f]{40}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
DEBIAN_UPSTREAM_RE = re.compile(r"^[0-9][A-Za-z0-9.+~]*$")
DEV_VERSION_RE = re.compile(
    r"^(?P<upstream>[0-9][A-Za-z0-9.+~]*)~dev(?P<sequence>[0-9]{4})"
    r"\+(?P<utc>[0-9]{14})\+g(?P<commit>[0-9a-f]{12})-(?P<revision>[1-9][0-9]*)$"
)


class EvidenceError(Exception):
    pass


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def archive_member_sha256(archive_path: Path, wanted: str) -> str:
    matches: list[tarfile.TarInfo] = []
    with tarfile.open(archive_path, mode="r:*") as archive:
        for member in archive.getmembers():
            name = member.name
            while name.startswith("./"):
                name = name[2:]
            if name == wanted:
                matches.append(member)
        if len(matches) != 1 or not matches[0].isfile():
            raise EvidenceError(f"source archive must contain one regular {wanted}")
        stream = archive.extractfile(matches[0])
        if stream is None:
            raise EvidenceError(f"cannot read {wanted} from source archive")
        digest = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
        return digest.hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise EvidenceError(f"cannot read JSON evidence {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise EvidenceError(f"JSON evidence is not an object: {path}")
    return value


def require_sha(value: Any, label: str, pattern: re.Pattern[str] = SHA256_RE) -> str:
    if not isinstance(value, str) or not pattern.fullmatch(value):
        raise EvidenceError(f"{label} must be a lowercase full-length hexadecimal digest")
    return value


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


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, path)


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


def safe_binary_from_deb(deb_path: Path) -> str:
    result = subprocess.run(
        ["dpkg-deb", "--fsys-tarfile", str(deb_path)],
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise EvidenceError(f"dpkg-deb could not read package data archive: {result.stderr.decode(errors='replace').strip()}")
    matches: list[tarfile.TarInfo] = []
    seen: set[str] = set()
    with tarfile.open(fileobj=io.BytesIO(result.stdout), mode="r:*") as archive:
        for member in archive.getmembers():
            raw = member.name
            while raw.startswith("./"):
                raw = raw[2:]
            if raw.startswith("/") or "\\" in raw or "\x00" in raw:
                raise EvidenceError(f"unsafe package path in data archive: {member.name!r}")
            normalized = posixpath.normpath(raw)
            if normalized == ".." or normalized.startswith("../"):
                raise EvidenceError(f"path escapes package root: {member.name!r}")
            if normalized in seen:
                raise EvidenceError(f"duplicate package path: {normalized}")
            seen.add(normalized)
            if member.issym() or member.islnk() or not (member.isdir() or member.isfile()):
                raise EvidenceError(f"package contains a link or special file: {normalized}")
            if normalized == "usr/bin/velnor-host":
                if not member.isfile():
                    raise EvidenceError("packaged controller binary is not a regular file")
                matches.append(member)
    if len(matches) != 1:
        raise EvidenceError("package must contain exactly one regular usr/bin/velnor-host")
    with tarfile.open(fileobj=io.BytesIO(result.stdout), mode="r:*") as archive:
        stream = archive.extractfile(matches[0])
        if stream is None:
            raise EvidenceError("cannot read packaged controller binary")
        digest = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
        return digest.hexdigest()


def verify_recorded_file(path_value: Any, expected_hash: Any, label: str) -> Path:
    if not isinstance(path_value, str) or not path_value:
        raise EvidenceError(f"{label} path must be a non-empty string")
    path = Path(path_value)
    if path.is_symlink() or not path.is_file():
        raise EvidenceError(f"{label} must be an existing regular file")
    digest = require_sha(expected_hash, f"{label} SHA-256")
    if sha256_file(path) != digest:
        raise EvidenceError(f"{label} bytes do not match the recorded SHA-256")
    return path


def command_tokens(path: Path, digest: Any, label: str) -> list[str]:
    path = verify_recorded_file(str(path), digest, label)
    try:
        tokens = shlex.split(path.read_text(encoding="utf-8"))
    except (UnicodeDecodeError, ValueError) as exc:
        raise EvidenceError(f"{label} is not a valid recorded command line") from exc
    if not tokens:
        raise EvidenceError(f"{label} is empty")
    return tokens


def require_nonempty_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise EvidenceError(f"{label} must be a non-empty string")
    return value


def command_flag_value(tokens: list[str], flag: str, label: str) -> str:
    values: list[str] = []
    for index, token in enumerate(tokens):
        if token == flag:
            if index + 1 >= len(tokens):
                raise EvidenceError(f"{label} is missing a value after {flag}")
            values.append(tokens[index + 1])
        elif token.startswith(flag + "="):
            values.append(token[len(flag) + 1 :])
    if len(values) != 1 or not values[0]:
        raise EvidenceError(f"{label} must contain exactly one {flag} value")
    return values[0]


def cargo_arguments(tokens: list[str], label: str, subcommand: str) -> list[str]:
    cargo_index = next((i for i, token in enumerate(tokens) if Path(token).name == "cargo"), None)
    if cargo_index is None:
        raise EvidenceError(f"{label} does not invoke Cargo")
    args = tokens[cargo_index + 1 :]
    if args and args[0].startswith("+"):
        args = args[1:]
    if not args or args[0] != subcommand:
        raise EvidenceError(f"{label} must invoke cargo {subcommand}")
    return args[1:]


def command_env_value(tokens: list[str], name: str, label: str) -> str:
    cargo_index = next((i for i, token in enumerate(tokens) if Path(token).name == "cargo"), None)
    if cargo_index is None:
        raise EvidenceError(f"{label} does not invoke Cargo")
    values = [token[len(name) + 1 :] for token in tokens[:cargo_index] if token.startswith(name + "=")]
    if len(values) != 1 or not values[0]:
        raise EvidenceError(f"{label} must record exactly one {name} before Cargo")
    return values[0]


def validate_build_provenance(
    record: dict[str, Any], plan: dict[str, Any], cargo_deb_path: Path, cargo_deb_hash: str
) -> dict[str, Any]:
    if type(record.get("schema")) is not int or record["schema"] != 1:
        raise EvidenceError("provenance record schema must be integer 1")
    source = record.get("source")
    if not isinstance(source, dict):
        raise EvidenceError("provenance source must be an object")
    expected_source = plan.get("source", {})
    source_fields = (
        ("commit", "commit"),
        ("tree", "tree"),
        ("archive_sha256", "archive_sha256"),
        ("Cargo_lock_sha256", "cargo_lock_sha256"),
    )
    for record_key, plan_key in source_fields:
        if source.get(record_key) != expected_source.get(plan_key):
            raise EvidenceError(f"provenance source {record_key} does not match the version plan")

    build = record.get("build")
    if not isinstance(build, dict):
        raise EvidenceError("provenance build must be an object")
    if type(build.get("exit_code")) is not int or build["exit_code"] != 0:
        raise EvidenceError("build producer must record integer exit_code 0")
    build_log = verify_recorded_file(
        build.get("stdout_stderr_log"), build.get("stdout_stderr_sha256"), "build log"
    )
    build_command_file = Path(require_nonempty_string(build.get("command_file"), "build command_file"))
    build_command = command_tokens(
        build_command_file, build.get("command_sha256"), "build command file"
    )
    build_args = cargo_arguments(build_command, "build command", "build")
    if "--release" not in build_args:
        raise EvidenceError("build command must select the release profile")
    if "--frozen" not in build_args and not ("--locked" in build_args and "--offline" in build_args):
        raise EvidenceError("build command must use --frozen or both --locked and --offline")
    target = plan.get("package", {}).get("rust_target")
    if command_flag_value(build_args, "--target", "build command") != target:
        raise EvidenceError("build command target does not match the version plan")
    manifest = command_flag_value(build_args, "--manifest-path", "build command")
    if not manifest.endswith("/crates/tools/velnor-runner-cli/Cargo.toml"):
        raise EvidenceError("build command must target the Velnor CLI manifest")
    if command_flag_value(build_args, "--bin", "build command") != "velnor-host":
        raise EvidenceError("build command must select the velnor-host binary")

    toolchain = build.get("toolchain")
    if not isinstance(toolchain, dict):
        raise EvidenceError("build toolchain must be an object")
    cargo_version = require_nonempty_string(toolchain.get("cargo"), "toolchain cargo version")
    rustc_version = require_nonempty_string(toolchain.get("rustc"), "toolchain rustc version")
    if not re.match(r"^[0-9]+\.[0-9]+\.[0-9]+(?:\s|$)", cargo_version):
        raise EvidenceError("toolchain cargo must start with a semantic version")
    if not re.match(r"^[0-9]+\.[0-9]+\.[0-9]+(?:\s|$)", rustc_version):
        raise EvidenceError("toolchain rustc must start with a semantic version")
    if toolchain.get("profile") != "release":
        raise EvidenceError("toolchain profile must be the release profile")
    if toolchain.get("target") != target:
        raise EvidenceError("toolchain target does not match the version plan")
    features = toolchain.get("features")
    if not isinstance(features, list) or any(not isinstance(item, str) or not item for item in features):
        raise EvidenceError("toolchain features must be an explicit list of non-empty strings")
    if len(features) != len(set(features)):
        raise EvidenceError("toolchain features must not contain duplicates")
    feature_flags: list[str] = []
    for index, token in enumerate(build_args):
        if token == "--features":
            if index + 1 >= len(build_args):
                raise EvidenceError("build command has an empty --features value")
            feature_flags.extend(value for value in build_args[index + 1].split(",") if value)
        elif token.startswith("--features="):
            feature_flags.extend(value for value in token[len("--features=") :].split(",") if value)
    if feature_flags != features:
        raise EvidenceError("build command feature selection does not match the explicit toolchain feature list")

    binary = build.get("binary")
    if not isinstance(binary, dict):
        raise EvidenceError("provenance build.binary must be an object")
    binary_hash = require_sha(binary.get("sha256"), "built binary SHA-256")
    binary_path = verify_recorded_file(binary.get("path"), binary_hash, "built binary")
    package = record.get("package")
    expected_package = plan.get("package", {})
    if not isinstance(package, dict):
        raise EvidenceError("provenance package must be an object")
    if package.get("architecture") != expected_package.get("architecture"):
        raise EvidenceError("package architecture does not match the version plan")
    if type(package.get("exit_code")) is not int or package["exit_code"] != 0:
        raise EvidenceError("package producer must record integer exit_code 0")
    if package.get("no_build") is not True or package.get("no_strip") is not True:
        raise EvidenceError("package producer must record no_build=true and no_strip=true")
    if package.get("version") != expected_package.get("version"):
        raise EvidenceError("package producer version does not match the development version plan")
    if package.get("tool") != f"cargo-deb {TOOL_VERSION}":
        raise EvidenceError("package producer must identify the pinned cargo-deb version")
    if package.get("tool_binary_sha256") != cargo_deb_hash:
        raise EvidenceError("package producer tool digest does not match pinned cargo-deb")
    package_log = verify_recorded_file(
        package.get("stdout_stderr_log"), package.get("stdout_stderr_sha256"), "package log"
    )
    package_command_file = Path(require_nonempty_string(package.get("command_file"), "package command_file"))
    package_command = command_tokens(
        package_command_file, package.get("command_sha256"), "package command file"
    )
    if Path(package_command[0]).name != "env":
        raise EvidenceError("package command must use an explicit env invocation")
    search_path = command_env_value(package_command, "PATH", "package command")
    if str(cargo_deb_path.parent) not in search_path.split(":"):
        raise EvidenceError("package command PATH does not include the pinned cargo-deb executable directory")
    package_args = cargo_arguments(package_command, "package command", "deb")
    if "--no-build" not in package_args or "--no-strip" not in package_args:
        raise EvidenceError("package command must use --no-build and --no-strip")
    if "--locked" not in package_args:
        raise EvidenceError("package command must use the committed lockfile")
    if command_flag_value(package_args, "--target", "package command") != target:
        raise EvidenceError("package command target does not match the version plan")
    version_flags = [token for token in package_args if token == "--deb-version" or token.startswith("--deb-version=")]
    if version_flags:
        if command_flag_value(package_args, "--deb-version", "package command") != expected_package.get("version"):
            raise EvidenceError("package command Debian version does not match the version plan")
    else:
        upstream_default = expected_package.get("cargo_upstream_version_at_source")
        if not isinstance(upstream_default, str) or expected_package.get("version") != f"{upstream_default}-1":
            raise EvidenceError("package command must pin the planned version with --deb-version")
    if command_flag_value(package_args, "--manifest-path", "package command") != manifest:
        raise EvidenceError("package command manifest differs from the verified build manifest")
    output_dir = Path(command_flag_value(package_args, "--output", "package command")).resolve()
    package_path = verify_recorded_file(package.get("path"), package.get("sha256"), "Debian package")
    if package_path.parent.resolve() != output_dir:
        raise EvidenceError("package path is outside the recorded cargo-deb output directory")
    packaged_binary_hash = require_sha(package.get("packaged_binary_sha256"), "packaged binary SHA-256")
    if packaged_binary_hash != binary_hash:
        raise EvidenceError("package producer binary digest differs from the verified build binary")
    if package.get("architecture") not in TARGETS:
        raise EvidenceError("package architecture is not in the supported Debian target map")
    if plan.get("cargo_deb", {}).get("binary_sha256") != cargo_deb_hash:
        raise EvidenceError("package provenance tool differs from the version plan")
    return {
        "build_log": build_log,
        "build_command_file": build_command_file,
        "build_command": build_command,
        "cargo": cargo_version,
        "rustc": rustc_version,
        "profile": toolchain["profile"],
        "features": features,
        "binary_path": binary_path,
        "binary_sha256": binary_hash,
        "package_command_file": package_command_file,
        "package_command": package_command,
        "package_log": package_log,
        "package_path": package_path,
        "package_sha256": package["sha256"],
        "target": target,
    }


def verify_package(args: argparse.Namespace) -> dict[str, Any]:
    plan = load_json(args.plan)
    if plan.get("status") != "VERSION_PLAN_ONLY_NO_BINARY_OR_DEB":
        raise EvidenceError("version plan has an unexpected status")
    source = plan.get("source", {})
    package = plan.get("package", {})
    source_archive = Path(str(source.get("archive_path", "")))
    source_archive_hash = require_sha(source.get("archive_sha256"), "planned source archive SHA-256")
    if not source_archive.is_file() or sha256_file(source_archive) != source_archive_hash:
        raise EvidenceError("the exact source archive no longer matches the package plan")
    if archive_member_sha256(source_archive, "Cargo.lock") != source.get("cargo_lock_sha256"):
        raise EvidenceError("the Cargo.lock hash in the package plan differs from its source archive")
    cargo_deb = plan.get("cargo_deb", {})
    cargo_deb_path = Path(str(cargo_deb.get("binary_path", "")))
    if not cargo_deb_path.is_file() or sha256_file(cargo_deb_path) != cargo_deb.get("binary_sha256"):
        raise EvidenceError("the cargo-deb executable does not match the pinned tool record")
    record = load_json(args.binary_build_record)
    verified = validate_build_provenance(record, plan, cargo_deb_path, cargo_deb["binary_sha256"])
    binary_hash = verified["binary_sha256"]
    binary_path = verified["binary_path"]
    elf = subprocess.run(["readelf", "-h", str(binary_path)], capture_output=True, text=True, check=False)
    if elf.returncode != 0 or TARGETS[package["architecture"]][1] not in elf.stdout:
        raise EvidenceError("binary ELF machine does not match the planned Debian architecture")
    if "--install" in verified["package_command"] or "--no-install-dbgsym" in verified["package_command"]:
        raise EvidenceError("package command must not install the package")
    deb_path = args.deb.resolve(strict=True)
    if deb_path != verified["package_path"].resolve(strict=True):
        raise EvidenceError("requested .deb path differs from the provenance record")
    deb_sha = sha256_file(deb_path)
    if deb_sha != verified["package_sha256"]:
        raise EvidenceError("Debian package bytes differ from the provenance record")
    fields = subprocess.run(
        ["dpkg-deb", "--field", str(deb_path), "Package", "Version", "Architecture"],
        capture_output=True,
        text=True,
        check=False,
    )
    if fields.returncode != 0:
        raise EvidenceError(f"dpkg-deb could not read package control fields: {fields.stderr.strip()}")
    field_values = fields.stdout.splitlines()
    if field_values != [PACKAGE_NAME, package["version"], package["architecture"]]:
        raise EvidenceError("package control identity differs from the version plan")
    deb_binary_hash = safe_binary_from_deb(deb_path)
    if deb_binary_hash != binary_hash:
        raise EvidenceError("binary bytes inside .deb differ from the independently verified build binary")
    return {
        "evidence_id": f"PKG-PROVENANCE-{source['commit'][:12]}-{package['version']}",
        "status": "PASS_ARTIFACT_PROVENANCE_ONLY",
        "source": source,
        "binary": {
            "path": str(binary_path),
            "sha256": binary_hash,
            "architecture": package["architecture"],
            "target": package["rust_target"],
            "build_record_path": str(args.binary_build_record.resolve()),
            "build_record_sha256": sha256_file(args.binary_build_record),
            "build_command_file": str(verified["build_command_file"]),
            "build_log": str(verified["build_log"]),
            "cargo": verified["cargo"],
            "rustc": verified["rustc"],
            "profile": verified["profile"],
            "features": verified["features"],
            "lockfile_sha256": source["cargo_lock_sha256"],
        },
        "package": {
            "path": str(deb_path),
            "sha256": deb_sha,
            "name": PACKAGE_NAME,
            "version": package["version"],
            "architecture": package["architecture"],
            "binary_path_inside_package": "usr/bin/velnor-host",
            "binary_sha256_inside_package": deb_binary_hash,
        },
        "cargo_deb": plan.get("cargo_deb"),
        "package_command": verified["package_command"],
        "limits": [
            "This record proves source receipt, build-record, binary, and package-byte identity only.",
            "It does not qualify Linux daemon execution, drain, service lifecycle, runner execution, upgrade, rollback, or installation.",
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
