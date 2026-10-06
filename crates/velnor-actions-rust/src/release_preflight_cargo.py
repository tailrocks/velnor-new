"""Verified package inventory from Cargo's isolated source packager."""
from pathlib import Path
import hashlib
import io
import json
import subprocess
import tarfile
import tempfile


def _probe_packaged_metadata(data, name, version, directory, environment):
    destination = Path(directory) / "metadata" / name
    contents = {}
    prefix = f"{name}-{version}/"
    # inventory() has already rejected unsafe paths and nonregular members.
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for member in archive:
            relative = member.name[len(prefix):]
            stream = archive.extractfile(member)
            require(stream is not None, "metadata_archive_stream")
            content = stream.read()
            contents[relative] = content
            path = destination / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
    argv = ["cargo", "metadata", "--locked", "--offline", "--no-deps",
            "--format-version", "1", "--manifest-path", str(destination / "Cargo.toml"),
            "--config", 'build.rustc="rustc"', "--config", 'build.rustc-wrapper=""',
            "--config", 'build.rustc-workspace-wrapper=""']
    result = subprocess.run(argv, cwd=directory, env=environment,
                            capture_output=True, check=False, timeout=600)
    require(result.returncode == 0, "approved_cargo_metadata_failed")
    metadata = approved_publish_metadata(contents, json.loads(result.stdout), name, version)
    proofs = archive_dependency_proofs(contents, metadata)
    validate_archive_publish_metadata(contents, metadata, name, version, proofs)
    return metadata, proofs


def approved_package_inventory(approved, source, manifest, destination=Path("release-package")):
    with tempfile.TemporaryDirectory(prefix="velnor-approved-package-") as directory:
        argv = ["cargo", "package", "--locked", "--manifest-path", str(source / manifest),
                "--target-dir", directory, "--config", 'build.rustc="rustc"',
                "--config", 'build.rustc-wrapper=""',
                "--config", 'build.rustc-workspace-wrapper=""']
        for name in sorted(approved["packages"]):
            argv.extend(["--package", name])
        environment = _clean_environment()
        environment["CARGO_HOME"] = str(Path(directory) / "cargo-home")
        environment["RUSTUP_TOOLCHAIN"] = approved["tools"]["rust"]
        for key in ("GITHUB_ENV", "GITHUB_PATH", "GITHUB_OUTPUT", "GITHUB_STATE", "GITHUB_STEP_SUMMARY"):
            environment.pop(key, None)
        result = subprocess.run(argv, cwd=directory, env=environment,
                                capture_output=True, check=False, timeout=600)
        detail = getattr(result, "stderr", b"").decode("utf-8", errors="replace").splitlines()
        require(result.returncode == 0, "approved_cargo_package_failed:" +
                (detail[0][:160] if detail else "cargo_failed"))
        packages = {}
        archive_bytes = {}
        for name, version in approved["packages"].items():
            archive = Path(directory) / "package" / f"{name}-{version}.crate"
            require(archive.is_file() and not archive.is_symlink(), "approved_archive_missing")
            data = archive.read_bytes()
            archive_bytes[name] = data
            item = inventory(data, name, version, approved["source_sha"])
            metadata, proofs = _probe_packaged_metadata(data, name, version, directory, environment)
            item.update(archive_sha256=hashlib.sha256(data).hexdigest(),
                        publish_metadata=metadata,
                        cargo_dependency_proofs=proofs,
                        dependencies=sorted({dependency["name"] for dependency in metadata["deps"]
                                             if dependency["kind"] != "dev" and
                                             dependency["name"] in approved["packages"]}))
            packages[name] = item
        selected_publication_order(packages)
        archives = destination / "crates"
        archives.mkdir(parents=True, exist_ok=False)
        for name, version in approved["packages"].items():
            (archives / f"{name}-{version}.crate").write_bytes(archive_bytes[name])
    return packages
