#!/usr/bin/env python3
"""Measure the exact Rust component projection consumed by qualified-tool installs."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
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


def qualify(
    target_key: str,
    source_sha: str,
    run_id: str,
    run_attempt: str,
    version: str,
    manifest_url: str,
    manifest_sha256: str,
) -> dict[str, object]:
    target = TARGETS[target_key]
    if (
        not re.fullmatch(r"[0-9a-f]{40}", source_sha)
        or not run_id.isdigit()
        or not run_attempt.isdigit()
        or not re.fullmatch(r"\d+\.\d+\.\d+", version)
        or not re.fullmatch(r"[0-9a-f]{64}", manifest_sha256)
        or manifest_url != f"https://static.rust-lang.org/dist/channel-rust-{version}.toml"
    ):
        raise QualificationError("workflow source or run identity is malformed")
    with tempfile.TemporaryDirectory(prefix="velnor-rust-qualification-") as temporary:
        root = Path(temporary)
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
        environment = {"HOME": str(root), "PATH": f"{prefix / 'bin'}:{os.defpath}"}
        cargo_output = subprocess.run(
            [cargo_path, "--version"], check=True, capture_output=True, text=True, timeout=30, env=environment
        ).stdout.splitlines()[0]
        rustc_output = subprocess.run(
            [rustc_path, "-vV"], check=True, capture_output=True, text=True, timeout=30, env=environment
        ).stdout.rstrip("\n")
        if not cargo_output.startswith(f"cargo {version} ") or f"release: {version}" not in rustc_output.splitlines():
            raise QualificationError("installed Rust probes differ from the pinned version")
        if f"host: {target}" not in rustc_output.splitlines():
            raise QualificationError("installed Rust host differs from the runner target")
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
            "schema": 1,
            "source": {
                "repository": os.environ.get("GITHUB_REPOSITORY", ""),
                "sha": source_sha,
                "workflow_sha": os.environ.get("GITHUB_WORKFLOW_SHA", ""),
            },
            "run": {"id": run_id, "attempt": run_attempt},
            "tool": {
                "id": "rust",
                "backend": {"kind": "core", "tool": "rust"},
                "version": version,
                "options": {"kind": "rust", "components": [], "targets": []},
                "depends_on": [],
                "platform": target_key,
                "artifacts": [{"url": item["url"], "sha256": item["sha256"]} for item in artifacts],
                "dependency_artifacts": [],
                "install_tree_sha256": canonical_tree_sha256(tree_entries(prefix)),
                "executables": executables,
            },
            "manifest": {"url": manifest_url, "sha256": manifest_sha},
        }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--manifest-url", required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--platform", choices=sorted(TARGETS), required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source_sha = os.environ.get("GITHUB_SHA", "")
    checked_out = subprocess.run(
        ["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True
    ).stdout.strip()
    if source_sha != checked_out:
        raise QualificationError("checkout does not match the dispatched workflow SHA")
    receipt = qualify(
        args.platform,
        source_sha,
        os.environ["GITHUB_RUN_ID"],
        os.environ["GITHUB_RUN_ATTEMPT"],
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
