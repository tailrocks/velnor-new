"""Validate a no-build Debian package against its recorded source and binary."""

from __future__ import annotations

import hashlib
import io
import posixpath
import re
import subprocess
import tarfile
from pathlib import Path
from typing import Any

from debian_package_common import (
    PACKAGE_NAME,
    TARGETS,
    TOOL_VERSION,
    EvidenceError,
    archive_member_sha256,
    load_json,
    require_sha,
    sha256_file,
)
from debian_package_command import (
    cargo_arguments,
    command_env_value,
    command_flag_value,
    command_tokens,
    require_nonempty_string,
    verify_recorded_file,
)


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







def validate_source_record(record: dict[str, Any], plan: dict[str, Any]) -> None:
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


def validate_build_command(
    build: dict[str, Any], plan: dict[str, Any]
) -> tuple[Path, Path, list[str], list[str], str, str]:
    if type(build.get("exit_code")) is not int or build["exit_code"] != 0:
        raise EvidenceError("build producer must record integer exit_code 0")
    build_log = verify_recorded_file(
        build.get("stdout_stderr_log"), build.get("stdout_stderr_sha256"), "build log"
    )
    command_file = Path(require_nonempty_string(build.get("command_file"), "build command_file"))
    command = command_tokens(command_file, build.get("command_sha256"), "build command file")
    args = cargo_arguments(command, "build command", "build")
    if "--release" not in args:
        raise EvidenceError("build command must select the release profile")
    if "--frozen" not in args and not ("--locked" in args and "--offline" in args):
        raise EvidenceError("build command must use --frozen or both --locked and --offline")
    target = plan.get("package", {}).get("rust_target")
    if command_flag_value(args, "--target", "build command") != target:
        raise EvidenceError("build command target does not match the version plan")
    manifest = command_flag_value(args, "--manifest-path", "build command")
    if not manifest.endswith("/crates/tools/velnor-runner-cli/Cargo.toml"):
        raise EvidenceError("build command must target the Velnor CLI manifest")
    if command_flag_value(args, "--bin", "build command") != "velnor-host":
        raise EvidenceError("build command must select the velnor-host binary")
    return build_log, command_file, command, args, target, manifest


def validate_toolchain(
    build: dict[str, Any], build_args: list[str], target: str
) -> dict[str, Any]:
    toolchain = build.get("toolchain")
    if not isinstance(toolchain, dict):
        raise EvidenceError("build toolchain must be an object")
    cargo_version = require_nonempty_string(toolchain.get("cargo"), "toolchain cargo version")
    rustc_version = require_nonempty_string(toolchain.get("rustc"), "toolchain rustc version")
    version_pattern = r"^[0-9]+\.[0-9]+\.[0-9]+(?:\s|$)"
    if not re.match(version_pattern, cargo_version):
        raise EvidenceError("toolchain cargo must start with a semantic version")
    if not re.match(version_pattern, rustc_version):
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
    return {
        "cargo": cargo_version,
        "rustc": rustc_version,
        "profile": toolchain["profile"],
        "features": features,
    }


def validate_binary_record(build: dict[str, Any]) -> tuple[Path, str]:
    binary = build.get("binary")
    if not isinstance(binary, dict):
        raise EvidenceError("provenance build.binary must be an object")
    binary_hash = require_sha(binary.get("sha256"), "built binary SHA-256")
    binary_path = verify_recorded_file(binary.get("path"), binary_hash, "built binary")
    return binary_path, binary_hash


def validate_package_record(
    package: Any,
    expected_package: dict[str, Any],
    cargo_deb_path: Path,
    cargo_deb_hash: str,
    plan: dict[str, Any],
    target: str,
    manifest: str,
    binary_hash: str,
) -> dict[str, Any]:
    if not isinstance(package, dict):
        raise EvidenceError("provenance package must be an object")
    if package.get("architecture") != expected_package.get("architecture"):
        raise EvidenceError("package architecture does not match the version plan")
    if type(package.get("exit_code")) is not int or package.get("exit_code") != 0:
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
    package_args = validate_package_command(
        package_command, cargo_deb_path, expected_package, target, manifest
    )
    package_path = verify_package_output(package, package_args)
    packaged_binary_hash = require_sha(package.get("packaged_binary_sha256"), "packaged binary SHA-256")
    if packaged_binary_hash != binary_hash:
        raise EvidenceError("package producer binary digest differs from the verified build binary")
    if package.get("architecture") not in TARGETS:
        raise EvidenceError("package architecture is not in the supported Debian target map")
    if plan.get("cargo_deb", {}).get("binary_sha256") != cargo_deb_hash:
        raise EvidenceError("package provenance tool differs from the version plan")
    return {
        "package_command_file": package_command_file,
        "package_command": package_command,
        "package_log": package_log,
        "package_path": package_path,
        "package_sha256": package["sha256"],
    }


def validate_package_command(
    package_command: list[str],
    cargo_deb_path: Path,
    expected_package: dict[str, Any],
    target: str,
    manifest: str,
) -> list[str]:
    if Path(package_command[0]).name != "env":
        raise EvidenceError("package command must use an explicit env invocation")
    search_path = command_env_value(package_command, "PATH", "package command")
    if str(cargo_deb_path.parent) not in search_path.split(":"):
        raise EvidenceError("package command PATH does not include the pinned cargo-deb executable directory")
    args = cargo_arguments(package_command, "package command", "deb")
    if "--no-build" not in args or "--no-strip" not in args:
        raise EvidenceError("package command must use --no-build and --no-strip")
    if "--locked" not in args:
        raise EvidenceError("package command must use the committed lockfile")
    if command_flag_value(args, "--target", "package command") != target:
        raise EvidenceError("package command target does not match the version plan")
    version_flags = [token for token in args if token == "--deb-version" or token.startswith("--deb-version=")]
    if version_flags:
        if command_flag_value(args, "--deb-version", "package command") != expected_package.get("version"):
            raise EvidenceError("package command Debian version does not match the version plan")
    else:
        upstream_default = expected_package.get("cargo_upstream_version_at_source")
        if not isinstance(upstream_default, str) or expected_package.get("version") != f"{upstream_default}-1":
            raise EvidenceError("package command must pin the planned version with --deb-version")
    if command_flag_value(args, "--manifest-path", "package command") != manifest:
        raise EvidenceError("package command manifest differs from the verified build manifest")
    return args


def verify_package_output(
    package: dict[str, Any], package_args: list[str]
) -> Path:
    output_dir = Path(command_flag_value(package_args, "--output", "package command")).resolve()
    package_path = verify_recorded_file(package.get("path"), package.get("sha256"), "Debian package")
    if package_path.parent.resolve() != output_dir:
        raise EvidenceError("package path is outside the recorded cargo-deb output directory")
    return package_path


def validate_build_provenance(
    record: dict[str, Any], plan: dict[str, Any], cargo_deb_path: Path, cargo_deb_hash: str
) -> dict[str, Any]:
    validate_source_record(record, plan)
    build = record.get("build")
    if not isinstance(build, dict):
        raise EvidenceError("provenance build must be an object")
    build_log, build_command_file, build_command, build_args, target, manifest = (
        validate_build_command(build, plan)
    )
    toolchain = validate_toolchain(build, build_args, target)
    binary_path, binary_hash = validate_binary_record(build)
    expected_package = plan.get("package", {})
    package_info = validate_package_record(
        record.get("package"), expected_package, cargo_deb_path, cargo_deb_hash,
        plan, target, manifest, binary_hash,
    )
    return {
        "build_log": build_log,
        "build_command_file": build_command_file,
        "build_command": build_command,
        **toolchain,
        "binary_path": binary_path,
        "binary_sha256": binary_hash,
        **package_info,
        "target": target,
    }

def read_deb_identity(deb_path: Path) -> list[str]:
    values: list[str] = []
    for field_name in ("Package", "Version", "Architecture"):
        result = subprocess.run(
            ["dpkg-deb", "--field", str(deb_path), field_name],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            raise EvidenceError(
                f"dpkg-deb could not read package control field {field_name}: {result.stderr.strip()}"
            )
        value = result.stdout.rstrip("\n")
        if not value or "\n" in value or "\r" in value:
            raise EvidenceError(f"dpkg-deb returned an invalid {field_name} control value")
        values.append(value)
    return values

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
    field_values = read_deb_identity(deb_path)
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
