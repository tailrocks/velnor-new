# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import gzip
import hashlib
import json
from pathlib import PurePosixPath
import re
import tarfile
import zlib

PLATFORM = {"architecture": "amd64", "os": "linux"}
MAX_ARCHIVE_BYTES = 4 * 1024**3
MAX_ARCHIVE_CONTENT_BYTES = 4 * 1024**3
MAX_ARCHIVE_MEMBERS = 128
MAX_BLOB_BYTES = 1024**3
MAX_UNCOMPRESSED_BYTES = 4 * 1024**3
MAX_LAYER_MEMBERS = 1_000_000
MAX_TAR_EXTENSION_BYTES = 1024**2
MAX_TAR_EXTENSION_TOTAL_BYTES = 4 * 1024**2
LAYER_MEDIA_TYPES = {
    "application/vnd.oci.image.layer.v1.tar",
    "application/vnd.oci.image.layer.v1.tar+gzip",
    "application/vnd.oci.image.layer.nondistributable.v1.tar",
    "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip",
}


class AuthorityError(ValueError):
    pass


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise AuthorityError(f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def _json_bytes(data, label):
    try:
        return json.loads(data, object_pairs_hook=_unique_object, parse_constant=_reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AuthorityError(f"{label} JSON has an incorrect format.") from error


def _reject_constant(value):
    raise AuthorityError(f"JSON constant has an incorrect value: {value}.")


def _sha(data):
    return hashlib.sha256(data).hexdigest()


def _hash_stream(stream, byte_limit=None):
    digest = hashlib.sha256()
    size = 0
    while chunk := stream.read(1024 * 1024):
        digest.update(chunk)
        size += len(chunk)
        if byte_limit is not None and size > byte_limit:
            raise AuthorityError("OCI archive content exceeds its size limit.")
    return digest.hexdigest(), size


def _safe_member(name):
    path = PurePosixPath(name)
    normalized = str(path)
    if path.is_absolute() or ".." in path.parts or normalized != name or name in {"", "."}:
        raise AuthorityError("OCI archive contains a path that is not safe.")
    return name


def _tar_member_size(header):
    field = header[124:136]
    if field[0] & 0x80:
        value = int.from_bytes(field, "big", signed=False) & ((1 << (8 * len(field) - 1)) - 1)
    else:
        try:
            value = int(field.strip(b"\0 ") or b"0", 8)
        except ValueError as error:
            raise AuthorityError("Tar archive has an incorrect member size.") from error
    if value < 0:
        raise AuthorityError("Tar archive has a negative member size.")
    return value


def _scan_tar_extensions(stream):
    extension_total = 0
    while header := stream.read(512):
        if len(header) != 512:
            return
        if header == bytes(512):
            return
        size = _tar_member_size(header)
        if header[156:157] in {b"x", b"g", b"L", b"K"}:
            if size > MAX_TAR_EXTENSION_BYTES:
                raise AuthorityError("Tar extension header exceeds its size limit.")
            extension_total += size
            if extension_total > MAX_TAR_EXTENSION_TOTAL_BYTES:
                raise AuthorityError("Tar extension headers exceed their total size limit.")
        remaining = (size + 511) // 512 * 512
        while remaining:
            chunk = stream.read(min(1024 * 1024, remaining))
            if not chunk:
                return
            remaining -= len(chunk)


def _scan_archive_extensions(path):
    try:
        with path.open("rb") as source:
            prefix = source.read(6)
            if prefix.startswith((b"\x1f\x8b", b"BZh", b"\xfd7zXZ\x00")):
                raise AuthorityError("OCI archive must use an uncompressed tar format.")
            source.seek(0)
            _scan_tar_extensions(_LayerHashReader(source, MAX_ARCHIVE_CONTENT_BYTES))
    except (OSError, EOFError, zlib.error) as error:
        raise AuthorityError("OCI archive extension scan failed.") from error


def _layer_path(name):
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts:
        raise AuthorityError("OCI layer has a path that is not safe.")
    normalized = str(path)
    if normalized in {"", "."}:
        return "."
    return normalized


class _LayerHashReader:
    def __init__(self, stream, byte_limit):
        self.stream = stream
        self.byte_limit = byte_limit
        self.digest = hashlib.sha256()
        self.size = 0

    def read(self, size=-1):
        data = self.stream.read(size)
        self.size += len(data)
        if self.size > self.byte_limit:
            raise AuthorityError("OCI image layers exceed their uncompressed size limit.")
        self.digest.update(data)
        return data


def _load_archive(path):
    json_files = {}
    blob_files = {}
    seen = set()
    content_size = 0
    if path.is_symlink() or not path.is_file():
        raise AuthorityError("OCI archive must be a regular file.")
    if path.stat().st_size > MAX_ARCHIVE_BYTES:
        raise AuthorityError("OCI archive exceeds its file size limit.")
    _scan_archive_extensions(path)
    try:
        archive = tarfile.open(path, mode="r:")
    except (tarfile.TarError, OSError) as error:
        raise AuthorityError("Cannot read OCI archive.") from error
    with archive:
        for member in archive:
            if len(seen) >= MAX_ARCHIVE_MEMBERS:
                raise AuthorityError("OCI archive has too many entries.")
            directory = member.isdir()
            raw_name = member.name.removesuffix("/") if directory else member.name
            name = _safe_member(raw_name)
            if len(name) > 1024:
                raise AuthorityError("OCI archive path exceeds its size limit.")
            if name in seen:
                raise AuthorityError(f"Duplicate archive path: {name}")
            seen.add(name)
            if directory:
                if name not in {"blobs", "blobs/sha256"}:
                    raise AuthorityError(f"Unexpected OCI archive directory: {name}")
                continue
            if not member.isfile():
                raise AuthorityError(f"OCI archive entry is not a regular file: {name}")
            if member.size < 0 or member.size > MAX_BLOB_BYTES:
                raise AuthorityError("OCI archive entry exceeds its size limit.")
            content_size += member.size
            if content_size > MAX_ARCHIVE_CONTENT_BYTES:
                raise AuthorityError("OCI archive content exceeds its size limit.")
            if name not in {"oci-layout", "index.json"} and not re.fullmatch(
                r"blobs/sha256/[0-9a-f]{64}", name
            ):
                raise AuthorityError(f"Unexpected OCI archive path: {name}")
            stream = archive.extractfile(member)
            if stream is None:
                raise AuthorityError("Cannot read OCI archive entry.")
            with stream:
                if name.startswith("blobs/"):
                    if len(blob_files) >= 64:
                        raise AuthorityError("OCI archive has too many blobs.")
                    blob_files[name.removeprefix("blobs/sha256/")] = member.size
                else:
                    if member.size > 1024 * 1024:
                        raise AuthorityError(f"OCI metadata file is too large: {name}")
                    json_files[name] = stream.read()
    if set(json_files) != {"oci-layout", "index.json"}:
        raise AuthorityError("OCI archive must contain oci-layout and index.json.")
    return json_files, blob_files


def verify_oci_archive(path):
    json_files, blobs = _load_archive(path)
    layout = _json_bytes(json_files["oci-layout"], "OCI layout")
    if set(layout) != {"imageLayoutVersion"}:
        raise AuthorityError("OCI layout has unknown or missing fields.")
    if layout["imageLayoutVersion"] != "1.0.0":
        raise AuthorityError("Unsupported OCI layout version.")

    index = _json_bytes(json_files["index.json"], "OCI index")
    if not isinstance(index, dict) or set(index) - {"schemaVersion", "mediaType", "manifests", "annotations"}:
        raise AuthorityError("OCI index has unknown fields.")
    if index.get("schemaVersion") != 2 or not isinstance(index.get("manifests"), list):
        raise AuthorityError("OCI index schema is incorrect.")
    manifests = index["manifests"]
    if len(manifests) != 1:
        raise AuthorityError("OCI index must select one platform manifest.")
    descriptor = manifests[0]
    required = {"mediaType", "digest", "size", "platform"}
    if not isinstance(descriptor, dict) or not required.issubset(descriptor) or set(descriptor) - (
        required | {"annotations"}
    ):
        raise AuthorityError("OCI platform descriptor has unknown or missing fields.")
    if type(descriptor["size"]) is not int or descriptor["size"] < 1:
        raise AuthorityError("OCI manifest size is incorrect.")
    if index.get("mediaType", "application/vnd.oci.image.index.v1+json") != "application/vnd.oci.image.index.v1+json":
        raise AuthorityError("OCI index media type is incorrect.")
    if descriptor["platform"] != PLATFORM:
        raise AuthorityError("OCI archive platform is not linux/amd64.")
    manifest_digest = _digest(descriptor["digest"], "Manifest digest")
    manifest_hex = manifest_digest.removeprefix("sha256:")
    manifest_size = blobs.get(manifest_hex)
    if manifest_size is None:
        raise AuthorityError("OCI manifest blob is missing.")
    if manifest_size != descriptor["size"]:
        raise AuthorityError("OCI manifest size is not the same as its descriptor size.")
    manifest_raw = _read_blob(path, manifest_hex)
    if _sha(manifest_raw) != manifest_hex:
        raise AuthorityError("OCI manifest blob has an incorrect digest.")
    manifest = _json_bytes(manifest_raw, "OCI manifest")
    if not isinstance(manifest, dict) or manifest.get("schemaVersion") != 2:
        raise AuthorityError("OCI manifest schema is incorrect.")
    if manifest.get("mediaType") != "application/vnd.oci.image.manifest.v1+json":
        raise AuthorityError("OCI manifest media type is incorrect.")
    if descriptor["mediaType"] != manifest["mediaType"]:
        raise AuthorityError("OCI index and manifest media types are different.")
    if set(manifest) - {"schemaVersion", "mediaType", "config", "layers", "annotations"}:
        raise AuthorityError("OCI manifest has unknown fields.")
    config = _descriptor(manifest.get("config"), "config")
    if config["mediaType"] != "application/vnd.oci.image.config.v1+json":
        raise AuthorityError("OCI image configuration media type is incorrect.")
    layers = manifest.get("layers")
    if not isinstance(layers, list) or not layers:
        raise AuthorityError("OCI manifest must contain image layers.")
    referenced = {}
    _record_blob_descriptor(referenced, {
        "hex": manifest_hex,
        "size": len(manifest_raw),
        "mediaType": manifest["mediaType"],
    })
    _record_blob_descriptor(referenced, config)
    layer_digests = []
    for layer in layers:
        entry = _descriptor(layer, "layer")
        if entry["mediaType"] not in LAYER_MEDIA_TYPES:
            raise AuthorityError("OCI layer media type is incorrect.")
        _record_blob_descriptor(referenced, entry)
        layer_digests.append(entry["digest"])
    if set(blobs) != set(referenced):
        raise AuthorityError("OCI archive has missing or unreferenced blobs.")
    for digest, (size, _media_type) in referenced.items():
        if blobs[digest] != size:
            raise AuthorityError("OCI blob size is not the same as its descriptor size.")
    config_raw = _read_blob(path, config["hex"])
    if _sha(config_raw) != config["hex"]:
        raise AuthorityError("OCI image configuration blob has an incorrect digest.")
    config_data = _json_bytes(config_raw, "OCI image configuration")
    if not isinstance(config_data, dict) or config_data.get("os") != "linux" or config_data.get("architecture") != "amd64":
        raise AuthorityError("OCI image configuration platform is not linux/amd64.")
    rootfs = config_data.get("rootfs")
    diff_ids = rootfs.get("diff_ids") if isinstance(rootfs, dict) else None
    if (
        not isinstance(rootfs, dict)
        or rootfs.get("type") != "layers"
        or not isinstance(diff_ids, list)
        or len(diff_ids) != len(layers)
        or any(not isinstance(item, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", item) for item in diff_ids)
    ):
        raise AuthorityError("OCI image configuration has incorrect DiffID data for its manifest layers.")
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
            raise AuthorityError("OCI layer DiffID is not the same as the digest of its uncompressed bytes.")
        uncompressed_total += layer_size
        member_total += member_count
        if member_total > MAX_LAYER_MEMBERS:
            raise AuthorityError("OCI image has too many layer paths.")
        if uncompressed_total > MAX_UNCOMPRESSED_BYTES:
            raise AuthorityError("OCI image layers exceed their uncompressed size limit.")
    for digest in referenced:
        if digest not in {manifest_hex, config["hex"]}:
            actual_digest, actual_size = _hash_blob(path, digest)
            if actual_digest != digest or actual_size != referenced[digest][0]:
                raise AuthorityError("OCI blob bytes are not the same as the bytes identified by their descriptor.")
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


def _descriptor(value, label):
    if not isinstance(value, dict) or not {"mediaType", "digest", "size"}.issubset(value):
        raise AuthorityError(f"OCI {label} descriptor is incorrect.")
    digest = _digest(value["digest"], f"OCI {label} digest")
    size = value["size"]
    if type(size) is not int or size < 0:
        raise AuthorityError(f"OCI {label} size is incorrect.")
    if not isinstance(value["mediaType"], str):
        raise AuthorityError(f"OCI {label} media type is incorrect.")
    return {"digest": digest, "hex": digest.removeprefix("sha256:"), "size": size, "mediaType": value["mediaType"]}


def _digest(value, label):
    if not isinstance(value, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", value):
        raise AuthorityError(f"{label} must be a lowercase SHA-256 digest.")
    return value


def _record_blob_descriptor(referenced, descriptor):
    metadata = (descriptor["size"], descriptor["mediaType"])
    existing = referenced.get(descriptor["hex"])
    if existing is not None and existing != metadata:
        raise AuthorityError("OCI descriptors for the same blob have different size or media type values.")
    referenced[descriptor["hex"]] = metadata


def _read_blob(path, digest):
    with tarfile.open(path, mode="r:") as archive:
        stream = archive.extractfile(f"blobs/sha256/{digest}")
        if stream is None:
            raise AuthorityError("OCI graph references a missing blob.")
        with stream:
            data = stream.read(64 * 1024 * 1024 + 1)
            if len(data) > 64 * 1024 * 1024:
                raise AuthorityError("OCI metadata blob is too large.")
            return data


def _hash_blob(path, digest):
    try:
        archive = tarfile.open(path, mode="r:")
    except (tarfile.TarError, OSError) as error:
        raise AuthorityError("Cannot read OCI archive.") from error
    with archive:
        stream = archive.extractfile(f"blobs/sha256/{digest}")
        if stream is None:
            raise AuthorityError("OCI graph references a missing blob.")
        with stream:
            return _hash_stream(stream, MAX_BLOB_BYTES)


def _layer_diff_id(path, digest, media_type, byte_limit, member_limit):
    try:
        archive = tarfile.open(path, mode="r:")
    except (tarfile.TarError, OSError) as error:
        raise AuthorityError("Cannot read OCI archive.") from error
    with archive:
        stream = archive.extractfile(f"blobs/sha256/{digest}")
        if stream is None:
            raise AuthorityError("OCI manifest references a missing layer.")
        with stream:
            try:
                first_stream = gzip.GzipFile(fileobj=stream) if media_type.endswith("+gzip") else stream
                _scan_tar_extensions(_LayerHashReader(first_stream, byte_limit))
                stream.seek(0)
                uncompressed = gzip.GzipFile(fileobj=stream) if media_type.endswith("+gzip") else stream
                reader = _LayerHashReader(uncompressed, byte_limit)
                paths = set()
                with tarfile.open(fileobj=reader, mode="r|") as layer_tar:
                    for member in layer_tar:
                        if len(paths) + 1 > member_limit:
                            raise AuthorityError("OCI layer has too many paths.")
                        normalized = _layer_path(member.name)
                        if normalized in paths:
                            raise AuthorityError(f"OCI layer repeats a path: {normalized}")
                        paths.add(normalized)
                while reader.read(1024 * 1024):
                    pass
                return reader.digest.hexdigest(), reader.size, len(paths)
            except (tarfile.TarError, OSError, EOFError, zlib.error) as error:
                raise AuthorityError("OCI layer is not a tar archive.") from error


def _hash_file(path):
    with path.open("rb") as stream:
        return _hash_stream(stream)
