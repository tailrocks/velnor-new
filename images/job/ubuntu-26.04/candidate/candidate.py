#!/usr/bin/env python3
"""Build an Ubuntu job image candidate. Verify its OCI archive."""

from __future__ import annotations

import argparse
import shutil
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tempfile
import tomllib
import zlib
from typing import Any


SCHEMA = "velnor.job-image-candidate.v1"
PLATFORM = {"architecture": "amd64", "os": "linux"}
MAX_ARCHIVE_BYTES = 4 * 1024**3
MAX_ARCHIVE_CONTENT_BYTES = 4 * 1024**3
MAX_ARCHIVE_MEMBERS = 128
MAX_BLOB_BYTES = 1024**3
MAX_UNCOMPRESSED_BYTES = 4 * 1024**3
MAX_LAYER_MEMBERS = 1_000_000
MAX_TAR_EXTENSION_BYTES = 1024**2
MAX_TAR_EXTENSION_TOTAL_BYTES = 4 * 1024**2
MAX_RECORD_BYTES = 1024**2
LAYER_MEDIA_TYPES = {
    "application/vnd.oci.image.layer.v1.tar",
    "application/vnd.oci.image.layer.v1.tar+gzip",
    "application/vnd.oci.image.layer.nondistributable.v1.tar",
    "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip",
}
ROOT = Path(__file__).resolve().parents[4]
INPUT_PATHS = (
    ".velnor/runner.lock",
    "images/job/ubuntu-26.04/Dockerfile",
    "images/job/ubuntu-26.04/README.md",
    "images/job/ubuntu-26.04/build-test.sh",
    "images/job/ubuntu-26.04/candidate/candidate.py",
)
RECORD_KEYS = {
    "schema", "source", "workflow", "platform", "build", "archive", "oci"
}
SOURCE_KEYS = {"commit", "ref", "inputs"}
WORKFLOW_KEYS = {
    "repository", "repository_id", "event", "workflow_ref", "authority_commit",
    "run_id", "run_attempt",
}
BUILD_KEYS = {
    "builder", "platform", "arguments", "exporter", "provenance",
    "compression", "force_compression",
}
ARCHIVE_KEYS = {"sha256", "size_bytes"}
OCI_KEYS = {
    "archive_index_digest", "archive_index_size_bytes", "platform_manifest_digest",
    "platform_manifest_size_bytes", "image_configuration_digest",
    "image_configuration_size_bytes", "layer_digests",
}


class CandidateError(ValueError):
    """The candidate input or archive is invalid."""


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise CandidateError(f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def _json_bytes(data: bytes, label: str) -> Any:
    try:
        return json.loads(data, object_pairs_hook=_unique_object, parse_constant=_reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise CandidateError(f"Invalid {label} JSON.") from error


def _reject_constant(value: str) -> None:
    raise CandidateError(f"Invalid JSON constant: {value}")


def _exact_keys(value: Any, expected: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != expected:
        raise CandidateError(f"{label} has unknown or missing fields.")
    return value


def _digest(value: Any, label: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", value):
        raise CandidateError(f"{label} must be a lowercase SHA-256 digest.")
    return value


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _hash_stream(stream: Any, byte_limit: int | None = None) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    while chunk := stream.read(1024 * 1024):
        digest.update(chunk)
        size += len(chunk)
        if byte_limit is not None and size > byte_limit:
            raise CandidateError("OCI archive content exceeds its size limit.")
    return digest.hexdigest(), size


def _safe_member(name: str) -> str:
    path = PurePosixPath(name)
    normalized = str(path)
    if path.is_absolute() or ".." in path.parts or normalized != name or name in {"", "."}:
        raise CandidateError("OCI archive contains an unsafe path.")
    return name


def _load_archive(path: Path) -> tuple[dict[str, bytes], dict[str, int]]:
    json_files: dict[str, bytes] = {}
    blob_files: dict[str, int] = {}
    seen: set[str] = set()
    content_size = 0
    if path.is_symlink() or not path.is_file():
        raise CandidateError("OCI archive must be a regular file.")
    if path.stat().st_size > MAX_ARCHIVE_BYTES:
        raise CandidateError("OCI archive exceeds its file size limit.")
    _scan_archive_extensions(path)
    try:
        archive = tarfile.open(path, mode="r:")
    except (tarfile.TarError, OSError) as error:
        raise CandidateError("Cannot read OCI archive.") from error
    with archive:
        for member in archive:
            if len(seen) >= MAX_ARCHIVE_MEMBERS:
                raise CandidateError("OCI archive has too many entries.")
            directory = member.isdir()
            raw_name = member.name.removesuffix("/") if directory else member.name
            name = _safe_member(raw_name)
            if len(name) > 1024:
                raise CandidateError("OCI archive path exceeds its size limit.")
            if name in seen:
                raise CandidateError(f"Duplicate archive path: {name}")
            seen.add(name)
            if directory:
                if name not in {"blobs", "blobs/sha256"}:
                    raise CandidateError(f"Unexpected OCI archive directory: {name}")
                continue
            if not member.isfile():
                raise CandidateError(f"OCI archive entry is not a regular file: {name}")
            if member.size < 0 or member.size > MAX_BLOB_BYTES:
                raise CandidateError("OCI archive entry exceeds its size limit.")
            content_size += member.size
            if content_size > MAX_ARCHIVE_CONTENT_BYTES:
                raise CandidateError("OCI archive content exceeds its size limit.")
            if name not in {"oci-layout", "index.json"} and not re.fullmatch(
                r"blobs/sha256/[0-9a-f]{64}", name
            ):
                raise CandidateError(f"Unexpected OCI archive path: {name}")
            stream = archive.extractfile(member)
            if stream is None:
                raise CandidateError("Cannot read OCI archive entry.")
            with stream:
                if name.startswith("blobs/"):
                    if len(blob_files) >= 64:
                        raise CandidateError("OCI archive has too many blobs.")
                    blob_files[name.removeprefix("blobs/sha256/")] = member.size
                else:
                    if member.size > 1024 * 1024:
                        raise CandidateError(f"OCI metadata file is too large: {name}")
                    json_files[name] = stream.read()
    if set(json_files) != {"oci-layout", "index.json"}:
        raise CandidateError("OCI archive must contain oci-layout and index.json.")
    return json_files, blob_files


def verify_oci_archive(path: Path) -> dict[str, Any]:
    json_files, blobs = _load_archive(path)
    layout = _json_bytes(json_files["oci-layout"], "OCI layout")
    _exact_keys(layout, {"imageLayoutVersion"}, "OCI layout")
    if layout["imageLayoutVersion"] != "1.0.0":
        raise CandidateError("Unsupported OCI layout version.")

    index = _json_bytes(json_files["index.json"], "OCI index")
    if not isinstance(index, dict) or set(index) - {"schemaVersion", "mediaType", "manifests", "annotations"}:
        raise CandidateError("OCI index has unknown fields.")
    if index.get("schemaVersion") != 2 or not isinstance(index.get("manifests"), list):
        raise CandidateError("OCI index schema is invalid.")
    manifests = index["manifests"]
    if len(manifests) != 1:
        raise CandidateError("OCI index must select one platform manifest.")
    descriptor = manifests[0]
    required = {"mediaType", "digest", "size", "platform"}
    if not isinstance(descriptor, dict) or not required.issubset(descriptor) or set(descriptor) - (
        required | {"annotations"}
    ):
        raise CandidateError("OCI platform descriptor has unknown or missing fields.")
    if type(descriptor["size"]) is not int or descriptor["size"] < 1:
        raise CandidateError("OCI manifest size is invalid.")
    if index.get("mediaType", "application/vnd.oci.image.index.v1+json") != "application/vnd.oci.image.index.v1+json":
        raise CandidateError("OCI index media type is invalid.")
    if descriptor["platform"] != PLATFORM:
        raise CandidateError("OCI archive does not contain exactly linux/amd64.")
    manifest_digest = _digest(descriptor["digest"], "Manifest digest")
    manifest_hex = manifest_digest.removeprefix("sha256:")
    manifest_size = blobs.get(manifest_hex)
    if manifest_size is None:
        raise CandidateError("OCI manifest blob is missing.")
    if manifest_size != descriptor["size"]:
        raise CandidateError("OCI manifest size does not match its descriptor.")
    manifest_raw = _read_blob(path, manifest_hex)
    if _sha(manifest_raw) != manifest_hex:
        raise CandidateError("OCI manifest blob has the wrong digest.")
    manifest = _json_bytes(manifest_raw, "OCI manifest")
    if not isinstance(manifest, dict) or manifest.get("schemaVersion") != 2:
        raise CandidateError("OCI manifest schema is invalid.")
    if manifest.get("mediaType") != "application/vnd.oci.image.manifest.v1+json":
        raise CandidateError("OCI manifest media type is invalid.")
    if descriptor["mediaType"] != manifest["mediaType"]:
        raise CandidateError("OCI index and manifest media types differ.")
    if set(manifest) - {"schemaVersion", "mediaType", "config", "layers", "annotations"}:
        raise CandidateError("OCI manifest has unknown fields.")
    config = _descriptor(manifest.get("config"), "config")
    if config["mediaType"] != "application/vnd.oci.image.config.v1+json":
        raise CandidateError("OCI image configuration media type is invalid.")
    layers = manifest.get("layers")
    if not isinstance(layers, list) or not layers:
        raise CandidateError("OCI manifest must contain image layers.")
    referenced: dict[str, int] = {manifest_hex: len(manifest_raw)}
    referenced[config["hex"]] = config["size"]
    layer_digests: list[str] = []
    for layer in layers:
        entry = _descriptor(layer, "layer")
        if entry["mediaType"] not in LAYER_MEDIA_TYPES:
            raise CandidateError("OCI layer media type is invalid.")
        if entry["hex"] in referenced:
            raise CandidateError("OCI graph contains a duplicate blob reference.")
        referenced[entry["hex"]] = entry["size"]
        layer_digests.append(entry["digest"])
    if set(blobs) != set(referenced):
        raise CandidateError("OCI archive has missing or unreferenced blobs.")
    for digest, size in referenced.items():
        if blobs[digest] != size:
            raise CandidateError("OCI blob size does not match its descriptor.")
    config_raw = _read_blob(path, config["hex"])
    if _sha(config_raw) != config["hex"]:
        raise CandidateError("OCI image configuration blob has the wrong digest.")
    config_data = _json_bytes(config_raw, "OCI image configuration")
    if not isinstance(config_data, dict) or config_data.get("os") != "linux" or config_data.get("architecture") != "amd64":
        raise CandidateError("OCI image configuration is not linux/amd64.")
    rootfs = config_data.get("rootfs")
    diff_ids = rootfs.get("diff_ids") if isinstance(rootfs, dict) else None
    if (
        not isinstance(rootfs, dict)
        or rootfs.get("type") != "layers"
        or not isinstance(diff_ids, list)
        or len(diff_ids) != len(layers)
        or any(not isinstance(item, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", item) for item in diff_ids)
    ):
        raise CandidateError("OCI image configuration layers do not match the manifest.")
    uncompressed_total = 0
    member_total = 0
    for layer, expected_diff_id in zip(layers, diff_ids):
        entry = _descriptor(layer, "layer")
        remaining = MAX_UNCOMPRESSED_BYTES - uncompressed_total
        actual_digest, layer_size, member_count = _layer_diff_id(
            path, entry["hex"], entry["mediaType"], remaining,
            MAX_LAYER_MEMBERS - member_total,
        )
        if f"sha256:{actual_digest}" != expected_diff_id:
            raise CandidateError("OCI layer DiffID does not match its uncompressed bytes.")
        uncompressed_total += layer_size
        member_total += member_count
        if member_total > MAX_LAYER_MEMBERS:
            raise CandidateError("OCI image has too many layer paths.")
        if uncompressed_total > MAX_UNCOMPRESSED_BYTES:
            raise CandidateError("OCI image layers exceed their uncompressed size limit.")
    for digest in referenced:
        if digest not in {manifest_hex, config["hex"]}:
            actual_digest, actual_size = _hash_blob(path, digest)
            if actual_digest != digest or actual_size != referenced[digest]:
                raise CandidateError("OCI blob bytes do not match their descriptor.")
    index_raw = json_files["index.json"]
    return {
        "archive_index_digest": _sha(index_raw),
        "archive_index_size_bytes": len(index_raw),
        "platform_manifest_digest": manifest_digest,
        "platform_manifest_size_bytes": len(manifest_raw),
        "image_configuration_digest": config["digest"],
        "image_configuration_size_bytes": len(config_raw),
        "layer_digests": layer_digests,
    }


def _descriptor(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict) or not {"mediaType", "digest", "size"}.issubset(value):
        raise CandidateError(f"OCI {label} descriptor is invalid.")
    digest = _digest(value["digest"], f"OCI {label} digest")
    size = value["size"]
    if type(size) is not int or size < 0:
        raise CandidateError(f"OCI {label} size is invalid.")
    if not isinstance(value["mediaType"], str):
        raise CandidateError(f"OCI {label} media type is invalid.")
    return {"digest": digest, "hex": digest.removeprefix("sha256:"), "size": size, "mediaType": value["mediaType"]}


def _read_blob(path: Path, digest: str) -> bytes:
    with tarfile.open(path, mode="r:") as archive:
        stream = archive.extractfile(f"blobs/sha256/{digest}")
        if stream is None:
            raise CandidateError("OCI graph references a missing blob.")
        with stream:
            data = stream.read(64 * 1024 * 1024 + 1)
            if len(data) > 64 * 1024 * 1024:
                raise CandidateError("OCI metadata blob is too large.")
            return data


def _hash_blob(path: Path, digest: str) -> tuple[str, int]:
    try:
        archive = tarfile.open(path, mode="r:")
    except (tarfile.TarError, OSError) as error:
        raise CandidateError("Cannot read OCI archive.") from error
    with archive:
        stream = archive.extractfile(f"blobs/sha256/{digest}")
        if stream is None:
            raise CandidateError("OCI graph references a missing blob.")
        with stream:
            return _hash_stream(stream, MAX_BLOB_BYTES)


def _layer_diff_id(
    path: Path, digest: str, media_type: str, byte_limit: int, member_limit: int
) -> tuple[str, int, int]:
    try:
        archive = tarfile.open(path, mode="r:")
    except (tarfile.TarError, OSError) as error:
        raise CandidateError("Cannot read OCI archive.") from error
    with archive:
        stream = archive.extractfile(f"blobs/sha256/{digest}")
        if stream is None:
            raise CandidateError("OCI manifest references a missing layer.")
        with stream:
            try:
                first_stream = gzip.GzipFile(fileobj=stream) if media_type.endswith("+gzip") else stream
                _scan_tar_extensions(_LayerHashReader(first_stream, byte_limit))
                stream.seek(0)
                uncompressed = gzip.GzipFile(fileobj=stream) if media_type.endswith("+gzip") else stream
                reader = _LayerHashReader(uncompressed, byte_limit)
                paths: set[str] = set()
                with tarfile.open(fileobj=reader, mode="r|") as layer_tar:
                    for member in layer_tar:
                        if len(paths) + 1 > member_limit:
                            raise CandidateError("OCI layer has too many paths.")
                        normalized = _layer_path(member.name)
                        if normalized in paths:
                            raise CandidateError(f"OCI layer repeats a path: {normalized}")
                        paths.add(normalized)
                while reader.read(1024 * 1024):
                    pass
                return reader.digest.hexdigest(), reader.size, len(paths)
            except (tarfile.TarError, OSError, EOFError, zlib.error) as error:
                raise CandidateError("OCI layer is not a valid tar archive.") from error


def _tar_member_size(header: bytes) -> int:
    field = header[124:136]
    if field[0] & 0x80:
        value = int.from_bytes(field, "big", signed=False) & ((1 << (8 * len(field) - 1)) - 1)
    else:
        try:
            value = int(field.strip(b"\0 ") or b"0", 8)
        except ValueError as error:
            raise CandidateError("Tar archive has an invalid member size.") from error
    if value < 0:
        raise CandidateError("Tar archive has a negative member size.")
    return value


def _scan_tar_extensions(stream: Any) -> None:
    extension_total = 0
    while header := stream.read(512):
        if len(header) != 512:
            return
        if header == bytes(512):
            return
        size = _tar_member_size(header)
        if header[156:157] in {b"x", b"g", b"L", b"K"}:
            if size > MAX_TAR_EXTENSION_BYTES:
                raise CandidateError("Tar extension header exceeds its size limit.")
            extension_total += size
            if extension_total > MAX_TAR_EXTENSION_TOTAL_BYTES:
                raise CandidateError("Tar extension headers exceed their total size limit.")
        remaining = (size + 511) // 512 * 512
        while remaining:
            chunk = stream.read(min(1024 * 1024, remaining))
            if not chunk:
                return
            remaining -= len(chunk)


def _scan_archive_extensions(path: Path) -> None:
    try:
        with path.open("rb") as source:
            prefix = source.read(6)
            if prefix.startswith((b"\x1f\x8b", b"BZh", b"\xfd7zXZ\x00")):
                raise CandidateError("OCI archive must use an uncompressed tar format.")
            source.seek(0)
            _scan_tar_extensions(_LayerHashReader(source, MAX_ARCHIVE_CONTENT_BYTES))
    except (OSError, EOFError, zlib.error) as error:
        raise CandidateError("OCI archive extension scan failed.") from error


def _layer_path(name: str) -> str:
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts:
        raise CandidateError("OCI layer has an unsafe path.")
    normalized = str(path)
    if normalized in {"", "."}:
        return "."
    return normalized


class _LayerHashReader:
    def __init__(self, stream: Any, byte_limit: int) -> None:
        self.stream = stream
        self.byte_limit = byte_limit
        self.digest = hashlib.sha256()
        self.size = 0

    def read(self, size: int = -1) -> bytes:
        data = self.stream.read(size)
        self.size += len(data)
        if self.size > self.byte_limit:
            raise CandidateError("OCI image layers exceed their uncompressed size limit.")
        self.digest.update(data)
        return data


def _source_inputs(root: Path) -> dict[str, str]:
    return {name: _sha((root / name).read_bytes()) for name in INPUT_PATHS}


def _verify_source_inputs(record: dict[str, Any], root: Path) -> None:
    if record["source"]["inputs"] != _source_inputs(root):
        raise CandidateError("Build input bytes changed after candidate creation.")


def _git_output(root: Path, *args: str) -> str:
    env = {name: value for name, value in os.environ.items() if not name.startswith("GIT_")}
    return subprocess.check_output(
        ["git", "-C", str(root), *args], text=True, env=env, timeout=15
    ).strip()


def _identity(root: Path, source_commit: str) -> dict[str, Any]:
    commit = _git_output(root, "rev-parse", "HEAD")
    if source_commit != commit or not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise CandidateError("Requested source commit must match the exact Git commit.")
    if _git_output(root, "status", "--porcelain", "--untracked-files=all"):
        raise CandidateError("Candidate builds require a clean Git worktree.")
    if os.environ.get("GITHUB_SHA") != commit:
        raise CandidateError("GitHub source commit does not match the checked out commit.")
    if _git_output(root, "show", f"{commit}:images/job/ubuntu-26.04/Dockerfile") != (root / INPUT_PATHS[1]).read_text().rstrip("\n"):
        raise CandidateError("Dockerfile bytes do not match the requested source commit.")
    return _source_inputs(root)


def _workflow_identity(env: dict[str, str]) -> dict[str, Any]:
    fields = {
        "repository": env.get("GITHUB_REPOSITORY", ""),
        "repository_id": env.get("GITHUB_REPOSITORY_ID", ""),
        "event": env.get("GITHUB_EVENT_NAME", ""),
        "workflow_ref": env.get("GITHUB_WORKFLOW_REF", ""),
        "ref": env.get("GITHUB_REF", ""),
        "authority_commit": env.get("GITHUB_WORKFLOW_SHA", ""),
        "run_id": env.get("GITHUB_RUN_ID", ""),
        "run_attempt": env.get("GITHUB_RUN_ATTEMPT", ""),
    }
    if not fields["ref"].startswith("refs/") or "\n" in fields["ref"]:
        raise CandidateError("GitHub source ref is missing or invalid.")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", fields["repository"]):
        raise CandidateError("GitHub repository identity is invalid.")
    if not fields["repository_id"].isdigit() or int(fields["repository_id"]) < 1:
        raise CandidateError("GitHub repository ID is invalid.")
    if not re.fullmatch(r"[a-z_]+", fields["event"]):
        raise CandidateError("GitHub event name is invalid.")
    if not fields["workflow_ref"].startswith(f"{fields['repository']}/.github/workflows/") or "@refs/" not in fields["workflow_ref"]:
        raise CandidateError("GitHub workflow ref is invalid.")
    fields["repository_id"] = int(fields["repository_id"])
    if not re.fullmatch(r"[0-9a-f]{40}", fields["authority_commit"]):
        raise CandidateError("GitHub workflow authority commit is invalid.")
    for name in ("run_id", "run_attempt"):
        if not fields[name].isdigit() or int(fields[name]) < 1:
            raise CandidateError(f"GitHub {name} is invalid.")
        fields[name] = int(fields[name])
    return fields


def _output_path(root: Path, raw_path: str) -> Path:
    requested = Path(raw_path)
    if not requested.is_absolute():
        requested = root / requested
    absolute = requested.absolute()
    for component in (absolute, *absolute.parents):
        if component.is_symlink() and (component == absolute or not component.exists()):
            raise CandidateError("Candidate output paths cannot use symlinks.")
    resolved = requested.resolve()
    if resolved == root or root in resolved.parents:
        raise CandidateError("Candidate outputs must stay outside the source tree.")
    return resolved


def _publish_pair(staged_archive: Path, staged_record: Path, archive: Path, record: Path) -> None:
    linked: list[tuple[Path, Path]] = []
    try:
        for source, destination in ((staged_archive, archive), (staged_record, record)):
            os.link(source, destination, follow_symlinks=False)
            linked.append((source, destination))
    except OSError:
        for source, destination in reversed(linked):
            try:
                if os.path.samefile(source, destination):
                    destination.unlink()
            except FileNotFoundError:
                pass
        raise


def _write_private_file(path: Path, data: bytes) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(path, flags, 0o600)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


def build(args: argparse.Namespace) -> None:
    root = Path(args.root).resolve()
    inputs = _identity(root, args.source_commit)
    workflow = _workflow_identity(os.environ)
    builder, buildx_binary = _buildx_details(args.docker, root)
    archive_path = _output_path(root, args.archive)
    record_path = _output_path(root, args.record)
    if archive_path == record_path or archive_path.parent != record_path.parent:
        raise CandidateError("Candidate outputs must use separate files in one directory.")
    if not archive_path.parent.is_dir():
        raise CandidateError("Candidate output directory must exist.")
    if archive_path.exists() or record_path.exists():
        raise CandidateError("Candidate outputs must be new, separate files.")
    subprocess.run(
        ["bash", str(root / "images/job/ubuntu-26.04/build-test.sh")],
        check=True, cwd=root, timeout=300,
    )
    source_ref = workflow.pop("ref")
    dockerfile = root / "images/job/ubuntu-26.04/Dockerfile"
    with tempfile.TemporaryDirectory(
        prefix=".velnor-job-image-candidate-", dir=archive_path.parent
    ) as temporary:
        context = Path(temporary)
        (context / "Dockerfile").write_bytes(dockerfile.read_bytes())
        docker_config = context / "docker-config"
        docker_config.mkdir()
        plugin_dir = docker_config / "cli-plugins"
        plugin_dir.mkdir()
        isolated_buildx = plugin_dir / "docker-buildx"
        shutil.copyfile(buildx_binary, isolated_buildx)
        isolated_buildx.chmod(0o755)
        if _hash_file(isolated_buildx)[0] != tomllib.loads(
            (root / ".velnor/runner.lock").read_text()
        )["buildx_sha256"]:
            raise CandidateError("Isolated Buildx binary does not match .velnor/runner.lock.")
        staged_archive = context / "candidate.oci.tar"
        staged_record = context / "candidate.json"
        docker_env = os.environ.copy()
        docker_env["DOCKER_CONFIG"] = str(docker_config)
        for name in ("DOCKER_CONTEXT", "DOCKER_HOST", "BUILDX_BUILDER"):
            docker_env.pop(name, None)
        for name in tuple(docker_env):
            if any(
                marker in name.upper()
                for marker in ("TOKEN", "PASSWORD", "SECRET", "CREDENTIAL", "AUTH", "KEY")
            ):
                docker_env.pop(name)
        command = [
            args.docker, "buildx", "build", "--platform", "linux/amd64", "--file",
            str(context / "Dockerfile"), "--provenance=false", "--output",
            f"type=oci,dest={staged_archive},compression=gzip,force-compression=true",
        ]
        command.append(str(context))
        subprocess.run(command, check=True, cwd=root, env=docker_env, timeout=1800)
        oci = verify_oci_archive(staged_archive)
        archive_hash, archive_size = _hash_file(staged_archive)
        record = {
            "schema": SCHEMA,
            "source": {"commit": args.source_commit, "ref": source_ref, "inputs": inputs},
            "workflow": workflow,
            "platform": "linux/amd64",
            "build": {
                "builder": builder, "platform": "linux/amd64", "arguments": [],
                "exporter": "oci", "provenance": "disabled",
                "compression": "gzip", "force_compression": True,
            },
            "archive": {"sha256": archive_hash, "size_bytes": archive_size},
            "oci": oci,
        }
        validate_record(record)
        record_bytes = (json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n").encode()
        _write_private_file(staged_record, record_bytes)
        _publish_pair(staged_archive, staged_record, archive_path, record_path)


def _buildx_details(docker: str, root: Path) -> tuple[str, Path]:
    result = subprocess.check_output([docker, "buildx", "version"], text=True, timeout=30).strip()
    if not result:
        raise CandidateError("Docker Buildx did not report its version.")
    lock = tomllib.loads((root / ".velnor/runner.lock").read_text())
    match = re.fullmatch(r"github\.com/docker/buildx v(\d+\.\d+\.\d+)(?: .+)?", result)
    if match is None or match.group(1) != lock["buildx_version"]:
        raise CandidateError("Docker Buildx version does not match .velnor/runner.lock.")
    plugin_data = subprocess.check_output(
        [docker, "info", "--format", "{{json .ClientInfo.Plugins}}"],
        text=True, timeout=30,
    ).strip()
    try:
        plugins = json.loads(plugin_data)
    except json.JSONDecodeError as error:
        raise CandidateError("Docker did not report CLI plugin identities.") from error
    matches = [plugin for plugin in plugins if isinstance(plugin, dict) and plugin.get("Name") == "buildx"]
    if len(matches) != 1 or not isinstance(matches[0].get("Path"), str):
        raise CandidateError("Docker did not report one Buildx plugin path.")
    binary = Path(matches[0]["Path"])
    if binary.is_symlink() or not binary.is_file():
        raise CandidateError("Docker Buildx plugin path is not a regular file.")
    actual_hash, _ = _hash_file(binary)
    if actual_hash != lock["buildx_sha256"]:
        raise CandidateError("Docker Buildx binary does not match .velnor/runner.lock.")
    if matches[0].get("Version") != f"v{lock['buildx_version']}":
        raise CandidateError("Docker Buildx plugin metadata does not match .velnor/runner.lock.")
    return result, binary


def _hash_file(path: Path) -> tuple[str, int]:
    with path.open("rb") as stream:
        return _hash_stream(stream)


def _read_limited_file(path: Path, limit: int, label: str) -> bytes:
    if path.is_symlink() or not path.is_file():
        raise CandidateError(f"{label} must be a regular file.")
    if path.stat().st_size > limit:
        raise CandidateError(f"{label} exceeds its size limit.")
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise CandidateError(f"{label} exceeds its size limit.")
    return data


def validate_record(value: Any) -> dict[str, Any]:
    record = _exact_keys(value, RECORD_KEYS, "Candidate record")
    if record["schema"] != SCHEMA or record["platform"] != "linux/amd64":
        raise CandidateError("Candidate schema or platform is invalid.")
    source = _exact_keys(record["source"], SOURCE_KEYS, "Source")
    if not isinstance(source["commit"], str) or not re.fullmatch(r"[0-9a-f]{40}", source["commit"]) or not isinstance(source["ref"], str) or not source["ref"].startswith("refs/"):
        raise CandidateError("Source identity is invalid.")
    if not isinstance(source["inputs"], dict) or set(source["inputs"]) != set(INPUT_PATHS):
        raise CandidateError("Source input closure is incomplete or unknown.")
    for name, digest in source["inputs"].items():
        if not isinstance(name, str) or not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise CandidateError("Source input hash is invalid.")
    workflow = _exact_keys(record["workflow"], WORKFLOW_KEYS, "Workflow")
    if not isinstance(workflow["repository"], str) or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", workflow["repository"]):
        raise CandidateError("Workflow repository identity is invalid.")
    if type(workflow["repository_id"]) is not int or workflow["repository_id"] < 1:
        raise CandidateError("Workflow repository ID is invalid.")
    if not isinstance(workflow["event"], str) or not re.fullmatch(r"[a-z_]+", workflow["event"]):
        raise CandidateError("Workflow event is invalid.")
    if (
        not isinstance(workflow["workflow_ref"], str)
        or not workflow["workflow_ref"].startswith(f"{workflow['repository']}/.github/workflows/")
        or "@refs/" not in workflow["workflow_ref"]
    ):
        raise CandidateError("Workflow ref is invalid.")
    if not isinstance(workflow["authority_commit"], str) or not re.fullmatch(r"[0-9a-f]{40}", workflow["authority_commit"]):
        raise CandidateError("Workflow authority commit is invalid.")
    for name in ("run_id", "run_attempt"):
        if type(workflow[name]) is not int or workflow[name] < 1:
            raise CandidateError(f"Workflow {name} is invalid.")
    build = _exact_keys(record["build"], BUILD_KEYS, "Build")
    if (
        not isinstance(build["builder"], str)
        or re.fullmatch(r"github\.com/docker/buildx v\d+\.\d+\.\d+(?: .+)?", build["builder"]) is None
        or build["platform"] != "linux/amd64"
        or build["exporter"] != "oci"
        or build["provenance"] != "disabled"
        or build["compression"] != "gzip"
        or build["force_compression"] is not True
    ):
        raise CandidateError("Build identity is invalid.")
    if build["arguments"] != []:
        raise CandidateError("Build arguments must match the empty build argument list.")
    archive = _exact_keys(record["archive"], ARCHIVE_KEYS, "Archive")
    if not isinstance(archive["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", archive["sha256"]) or type(archive["size_bytes"]) is not int or archive["size_bytes"] < 1:
        raise CandidateError("Archive identity is invalid.")
    oci = _exact_keys(record["oci"], OCI_KEYS, "OCI")
    for name in ("archive_index_digest",):
        if not isinstance(oci[name], str) or not re.fullmatch(r"[0-9a-f]{64}", oci[name]):
            raise CandidateError(f"OCI {name} is invalid.")
    for name in (
        "archive_index_size_bytes", "platform_manifest_size_bytes",
        "image_configuration_size_bytes",
    ):
        if type(oci[name]) is not int or oci[name] < 1:
            raise CandidateError(f"OCI {name} is invalid.")
    _digest(oci["platform_manifest_digest"], "OCI platform manifest digest")
    _digest(oci["image_configuration_digest"], "OCI image configuration digest")
    if not isinstance(oci["layer_digests"], list) or not oci["layer_digests"]:
        raise CandidateError("OCI layer digest list is invalid.")
    for digest in oci["layer_digests"]:
        _digest(digest, "OCI layer digest")
    return record


def validate(args: argparse.Namespace) -> None:
    archive = Path(args.archive)
    record_path = Path(args.record)
    record = validate_record(
        _json_bytes(_read_limited_file(record_path, MAX_RECORD_BYTES, "Candidate record"), "candidate record")
    )
    actual_oci = verify_oci_archive(archive)
    archive_hash, archive_size = _hash_file(archive)
    if record["archive"] != {"sha256": archive_hash, "size_bytes": archive_size}:
        raise CandidateError("Candidate archive does not match its record.")
    if record["oci"] != actual_oci:
        raise CandidateError("OCI graph does not match its candidate record.")
    root = Path(args.root).resolve()
    _verify_source_inputs(record, root)
    workflow = _workflow_identity(os.environ)
    source_ref = workflow.pop("ref")
    if record["source"]["commit"] != os.environ.get("GITHUB_SHA") or record["source"]["ref"] != source_ref:
        raise CandidateError("Candidate source identity does not match the current GitHub job.")
    if record["workflow"] != workflow:
        raise CandidateError("Candidate workflow identity does not match the current GitHub job.")
    lock = tomllib.loads((root / ".velnor/runner.lock").read_text())
    if record["build"]["builder"] != _locked_buildx_identity(record["build"]["builder"], lock["buildx_version"]):
        raise CandidateError("Candidate Buildx identity does not match .velnor/runner.lock.")
    print("PASS: Candidate record matches source inputs and OCI archive.")


def _locked_buildx_identity(builder: str, expected: str) -> str:
    match = re.fullmatch(r"github\.com/docker/buildx v(\d+\.\d+\.\d+)(?: .+)?", builder)
    if match is None or match.group(1) != expected:
        raise CandidateError("Candidate Buildx identity does not match .velnor/runner.lock.")
    return builder


def main() -> None:
    parser = argparse.ArgumentParser(description="Build or verify an Ubuntu job image candidate.")
    commands = parser.add_subparsers(dest="command", required=True)
    build_parser = commands.add_parser("build")
    build_parser.add_argument("--root", default=str(ROOT))
    build_parser.add_argument("--source-commit", required=True)
    build_parser.add_argument("--archive", required=True)
    build_parser.add_argument("--record", required=True)
    build_parser.add_argument("--docker", default="docker")
    build_parser.set_defaults(function=build)
    validate_parser = commands.add_parser("validate")
    validate_parser.add_argument("--root", default=str(ROOT))
    validate_parser.add_argument("--archive", required=True)
    validate_parser.add_argument("--record", required=True)
    validate_parser.set_defaults(function=validate)
    args = parser.parse_args()
    args.function(args)


if __name__ == "__main__":
    try:
        main()
    except (
        CandidateError, OSError, subprocess.CalledProcessError,
        subprocess.TimeoutExpired, tarfile.TarError,
    ) as error:
        raise SystemExit(f"ERROR: {error}") from error
