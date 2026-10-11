#!/usr/bin/env python3
"""Measure the exact Rust component projection consumed by qualified-tool installs."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

from rust_toolchain_tree import (
    QualificationError,
    canonical_tree_sha256,
    component_root,
    digest_file,
    extract_archive,
    merge_component,
    tree_entries,
)

TARGETS = {
    "linux_x64": "x86_64-unknown-linux-gnu",
    "macos_arm64": "aarch64-apple-darwin",
}
MAX_ARCHIVE_BYTES = 1024 * 1024 * 1024


class RejectRedirects(urllib.request.HTTPRedirectHandler):
    """Reject redirects so only the pinned official Rust host is contacted."""

    def redirect_request(self, request, response, code, message, headers, new_url):
        raise QualificationError("Rust source redirected away from its pinned URL")


def fetch(url: str, destination: Path, expected_sha256: str | None = None) -> str:
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "https" or parsed.netloc != "static.rust-lang.org" or parsed.query or parsed.fragment:
        raise QualificationError("Rust source URL is outside the pinned official host")
    destination.parent.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha256()
    total = 0
    request = urllib.request.Request(url, headers={"User-Agent": "velnor-rust-qualification/1"})
    opener = urllib.request.build_opener(RejectRedirects())
    with opener.open(request, timeout=60) as response, destination.open("xb") as output:
        while chunk := response.read(1024 * 1024):
            total += len(chunk)
            if total > MAX_ARCHIVE_BYTES:
                raise QualificationError("Rust source exceeds the archive byte limit")
            digest.update(chunk)
            output.write(chunk)
    actual = digest.hexdigest()
    if expected_sha256 is not None and actual != expected_sha256:
        destination.unlink(missing_ok=True)
        raise QualificationError("Rust source SHA-256 differs from the pinned digest")
    return actual


def manifest_artifacts(manifest: dict[str, object], target: str, version: str) -> list[dict[str, str]]:
    packages = manifest.get("pkg")
    if not isinstance(packages, dict):
        raise QualificationError("Rust channel manifest has no package table")
    components = ["cargo", "rustc", f"rust-std-{target}"]
    artifacts = []
    for component in components:
        package_id = "rust-std" if component.startswith("rust-std-") else component
        metadata = packages.get(package_id)
        package_version = metadata.get("version") if isinstance(metadata, dict) else None
        if not isinstance(package_version, str) or not package_version:
            raise QualificationError(f"Rust channel package version mismatch: {package_id}")
        if package_id != "cargo" and package_version.split(" ", 1)[0] != version:
            raise QualificationError(f"Rust channel package version mismatch: {package_id}")
        targets = metadata.get("target")
        target_data = targets.get(target) if isinstance(targets, dict) else None
        if not isinstance(target_data, dict) or target_data.get("available") is not True:
            raise QualificationError(f"Rust component unavailable for target: {component}")
        url, checksum = target_data.get("xz_url"), target_data.get("xz_hash")
        if not isinstance(url, str) or not isinstance(checksum, str):
            raise QualificationError(f"Rust component source is incomplete: {component}")
        parsed = urllib.parse.urlsplit(url or "")
        expected_file = f"{package_id}-{version}-{target}.tar.xz"
        if (
            parsed.scheme != "https"
            or parsed.netloc != "static.rust-lang.org"
            or parsed.query
            or parsed.fragment
            or parsed.path.rsplit("/", 1)[-1] != expected_file
            or not re.fullmatch(r"[0-9a-f]{64}", checksum or "")
        ):
            raise QualificationError(f"Rust component source is invalid: {component}")
        artifacts.append({"url": url, "sha256": checksum, "component": component})
    return sorted(artifacts, key=lambda item: item["url"])


def mbx_exec_argv(
    mbx_executable: Path,
    project_root: Path,
    executable: Path,
    arguments: tuple[str, ...],
) -> list[str]:
    if not mbx_executable.is_absolute() or not project_root.is_absolute() or not executable.is_absolute():
        raise QualificationError("MBX qualification paths must be absolute")
    return [
        str(mbx_executable),
        "exec",
        "--project-root",
        str(project_root),
        str(executable),
        *arguments,
    ]


def validate_mbx_version(output: str, version: str) -> None:
    if output.splitlines() != [f"mbx {version}"]:
        raise QualificationError("MBX executable differs from the pinned version")


def mbx_executor_identity(mbx_executable: Path, version: str) -> dict[str, str]:
    if (
        not mbx_executable.is_absolute()
        or not mbx_executable.is_file()
        or not os.access(mbx_executable, os.X_OK)
    ):
        raise QualificationError("pinned MBX executable is missing or not executable")
    return {
        "id": "mr-boxington",
        "version": version,
        "path": str(mbx_executable),
        "sha256": digest_file(mbx_executable),
    }


def ensure_mbx_executor_unchanged(
    expected: dict[str, str],
    actual: dict[str, str],
) -> None:
    if actual != expected:
        raise QualificationError("MBX executable identity changed during qualification")


def run_mbx_probe(
    mbx_executable: Path,
    project_root: Path,
    executable: Path,
    arguments: tuple[str, ...],
    environment: dict[str, str],
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        mbx_exec_argv(mbx_executable, project_root, executable, arguments),
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
        env=environment,
    )


def qualify(
    target_key: str,
    source_sha: str,
    run_id: str,
    run_attempt: str,
    mbx_executable: Path,
    mbx_version: str,
    project_root: Path,
    version: str,
    manifest_url: str,
    manifest_sha256: str,
) -> dict[str, object]:
    target = TARGETS[target_key]
    repository = os.environ.get("GITHUB_REPOSITORY", "")
    workflow_sha = os.environ.get("GITHUB_WORKFLOW_SHA", "")
    if (
        not re.fullmatch(r"[0-9a-f]{40}", source_sha)
        or not run_id.isdigit()
        or not run_attempt.isdigit()
        or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository)
        or not re.fullmatch(r"[0-9a-f]{40}", workflow_sha)
        or not re.fullmatch(r"\d+\.\d+\.\d+", version)
        or not re.fullmatch(r"\d+\.\d+\.\d+", mbx_version)
        or not re.fullmatch(r"[0-9a-f]{64}", manifest_sha256)
        or manifest_url != f"https://static.rust-lang.org/dist/channel-rust-{version}.toml"
        or not mbx_executable.is_absolute()
        or not project_root.is_absolute()
    ):
        raise QualificationError("workflow source or run identity is malformed")
    if not mbx_executable.is_file() or not os.access(mbx_executable, os.X_OK):
        raise QualificationError("pinned MBX executable is missing or not executable")
    if not project_root.is_dir():
        raise QualificationError("qualification project root is missing")
    executor = mbx_executor_identity(mbx_executable, mbx_version)
    with tempfile.TemporaryDirectory(prefix="velnor-rust-qualification-") as temporary:
        root = Path(temporary)
        environment = {"HOME": str(root), "PATH": os.defpath}
        mbx_output = subprocess.run(
            [str(mbx_executable), "--version"],
            check=True,
            capture_output=True,
            text=True,
            timeout=30,
            env=environment,
        ).stdout
        validate_mbx_version(mbx_output, mbx_version)
        manifest_path = root / "channel.toml"
        manifest_sha = fetch(manifest_url, manifest_path, manifest_sha256)
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        rustc = manifest.get("pkg", {}).get("rustc", {})
        if rustc.get("version", "").split(" ", 1)[0] != version:
            raise QualificationError("Rust manifest does not contain the pinned version")
        artifacts = manifest_artifacts(manifest, target, version)
        roots = []
        for index, artifact in enumerate(artifacts):
            archive = root / f"component-{index}.tar.xz"
            fetch(artifact["url"], archive, artifact["sha256"])
            unpacked = root / f"component-{index}"
            extract_archive(archive, unpacked)
            roots.append(component_root(unpacked))
        prefix = root / "prefix"
        prefix.mkdir()
        for artifact, component in zip(artifacts, roots, strict=True):
            merge_component(component, artifact["component"], prefix)
        cargo_path = prefix / "bin" / "cargo"
        rustc_path = prefix / "bin" / "rustc"
        environment["PATH"] = f"{prefix / 'bin'}:{os.defpath}"
        cargo_output = run_mbx_probe(
            mbx_executable, project_root, cargo_path, ("--version",), environment
        ).stdout.splitlines()[0]
        rustc_output = run_mbx_probe(
            mbx_executable, project_root, rustc_path, ("-vV",), environment
        ).stdout.rstrip("\n")
        if (
            not cargo_output.startswith(f"cargo {version} ")
            or f"release: {version}" not in rustc_output.splitlines()
        ):
            raise QualificationError("installed Rust probes differ from the pinned version")
        if f"host: {target}" not in rustc_output.splitlines():
            raise QualificationError("installed Rust host differs from the runner target")
        ensure_mbx_executor_unchanged(
            executor,
            mbx_executor_identity(mbx_executable, mbx_version),
        )
        executables = [
            {
                "name": name,
                "path": path,
                "sha256": digest_file(prefix / path),
                "probe": {"kind": kind, "expected": output},
            }
            for name, path, kind, output in [
                ("cargo", "bin/cargo", "version", cargo_output),
                ("rustc", "bin/rustc", "rustc_verbose", rustc_output),
            ]
        ]
        return {
            "schema": 2,
            "executor": executor,
            "source": {
                "repository": repository,
                "sha": source_sha,
                "workflow_sha": workflow_sha,
            },
            "run": {"id": run_id, "attempt": run_attempt},
            "tool": {
                "id": "rust",
                "backend": {"kind": "core", "tool": "rust"},
                "version": version,
                "options": {"kind": "rust", "components": [], "targets": []},
                "depends_on": [],
                "platform": target_key,
                "artifacts": [
                    {"url": item["url"], "sha256": item["sha256"]} for item in artifacts
                ],
                "dependency_artifacts": [],
                "install_tree_sha256": canonical_tree_sha256(tree_entries(prefix)),
                "executables": executables,
            },
            "manifest": {"url": manifest_url, "sha256": manifest_sha},
        }


def main() -> int:
    if sys.version_info < (3, 11):
        raise QualificationError("Python 3.11 or newer is required for TOML manifest validation")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--manifest-url", required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--mbx-executable", type=Path, required=True)
    parser.add_argument("--mbx-version", required=True)
    parser.add_argument("--project-root", type=Path, required=True)
    parser.add_argument("--platform", choices=sorted(TARGETS), required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source_sha = os.environ.get("GITHUB_SHA", "")
    if not args.mbx_executable.is_absolute() or not args.project_root.is_absolute():
        raise QualificationError("MBX executable and project root must be absolute")
    project_root = args.project_root.resolve(strict=True)
    workspace = os.environ.get("GITHUB_WORKSPACE", "")
    if not workspace or Path(workspace).resolve(strict=True) != project_root:
        raise QualificationError("qualification root differs from the runner workspace")
    checked_out = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
        cwd=project_root,
    ).stdout.strip()
    if source_sha != checked_out:
        raise QualificationError("checkout does not match the dispatched workflow SHA")
    receipt = qualify(
        args.platform,
        source_sha,
        os.environ["GITHUB_RUN_ID"],
        os.environ["GITHUB_RUN_ATTEMPT"],
        args.mbx_executable.resolve(strict=True),
        args.mbx_version,
        project_root,
        args.version,
        args.manifest_url,
        args.manifest_sha256,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(receipt, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, tarfile.TarError, tomllib.TOMLDecodeError, urllib.error.URLError, QualificationError) as error:
        raise SystemExit(f"Rust qualification failed: {error}") from error
