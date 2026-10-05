"""Data-only, bounded validation of an uncompressed OCI layout tar archive."""
import hashlib
import io
import json
import os
import re
import stat
from dataclasses import dataclass
from types import MappingProxyType

from oci_digest import (GateError, duplicate_object, lower_sha, need, oci_version, safe_arch, safe_id)
INDEX_MEDIA = "application/vnd.oci.image.index.v1+json"
MANIFEST_MEDIA = "application/vnd.oci.image.manifest.v1+json"
CONFIG_MEDIA = "application/vnd.oci.image.config.v1+json"
STATEMENT_MEDIA = "application/vnd.in-toto+json"
EMPTY_MEDIA = "application/vnd.oci.empty.v1+json"
ATTESTATION_ARTIFACT = "application/vnd.docker.attestation.manifest.v1+json"
PROVENANCE_TYPES = {"https://slsa.dev/provenance/v0.2", "https://slsa.dev/provenance/v1"}
SPDX_TYPE = "https://spdx.dev/Document"
BUILD_TYPES = {
    "https://mobyproject.org/buildkit@v1",
    "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md",
}
MAX_OCI_ENTRIES = 8192
MAX_OCI_METADATA = 8 * 1024 * 1024
MAX_OCI_BLOBS = 64 * 1024 * 1024 * 1024

def archive_identity(fd):
    info = os.fstat(fd)
    need(stat.S_ISREG(info.st_mode), "archive_regular_file")
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


class ArchiveStream(io.RawIOBase):
    """Independent bounded reads from a fixed descriptor, never reopen a path."""

    def __init__(self, blob):
        super().__init__()
        self.blob = blob
        self.position = 0

    def readable(self):
        return True

    def readinto(self, buffer):
        need(not self.closed, "archive_stream_closed")
        need(archive_identity(self.blob.fd) == self.blob.identity, "archive_changed")
        count = min(len(buffer), self.blob.size - self.position)
        data = os.pread(self.blob.fd, count, self.blob.offset + self.position)
        need(len(data) == count, "archive_truncated")
        buffer[:count] = data
        self.position += count
        return count


@dataclass(frozen=True)
class VerifiedBlob:
    digest: str
    size: int
    offset: int
    fd: int
    identity: tuple
    data: bytes | None = None

    def open(self):
        need(archive_identity(self.fd) == self.identity, "archive_changed")
        return io.BytesIO(self.data) if self.data is not None else io.BufferedReader(ArchiveStream(self))

    def metadata(self):
        need(self.size <= MAX_OCI_METADATA, "archive_metadata_bound")
        with self.open() as stream:
            return stream.read()


@dataclass(frozen=True)
class VerifiedArchive:
    digest: str
    image_id: str
    image: str
    version: str
    source_sha: str
    arch: str
    blobs: object
    manifests: tuple
    platform_digest: str
    fd: int

    def close(self):
        os.close(self.fd)

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

def archive_octal(field):
    value = field.rstrip(b"\x00 ").lstrip(b" ")
    need(all(48 <= byte <= 55 for byte in value), "archive_header_number")
    return int(value or b"0", 8)

def archive_name(header):
    def text_field(raw):
        value, separator, tail = raw.partition(b"\x00")
        need(not separator or not tail.strip(b"\x00"), "archive_header_text")
        try:
            return value.decode("ascii")
        except UnicodeDecodeError as error:
            raise GateError("archive_path") from error
    name = text_field(header[:100])
    prefix = text_field(header[345:500])
    need(not prefix and name, "archive_path")
    return name

def archive_pax(raw):
    need(raw, "archive_pax_record")
    values, position = {}, 0
    allowed = {"size", "mtime", "atime", "ctime", "uid", "gid", "uname", "gname"}
    while position < len(raw):
        space = raw.find(b" ", position, position + 12)
        need(space > position and raw[position:space].isdigit(), "archive_pax_record")
        length = int(raw[position:space])
        end = position + length
        need(space + 3 <= end <= len(raw) and raw[end - 1:end] == b"\n", "archive_pax_record")
        key, separator, value = raw[space + 1:end - 1].partition(b"=")
        need(separator and key.decode("ascii", errors="replace") in allowed, "archive_pax_key")
        need(key not in values and b"\x00" not in value, "archive_pax_record")
        values[key] = value
        position = end
    if b"size" in values:
        need(values[b"size"].isdigit() and len(values[b"size"]) <= 12, "archive_pax_size")
    return values

def archive_members(fd, identity):
    need(identity[2] <= MAX_OCI_BLOBS + MAX_OCI_ENTRIES * 1024, "archive_size_bound")
    members, position, total, count, pending = {}, 0, 0, 0, None
    while position < identity[2]:
        header = os.pread(fd, 512, position)
        need(len(header) == 512, "archive_header")
        if header == bytes(512):
            need(pending is None, "archive_pax_dangling")
            ending = os.pread(fd, 512, position + 512)
            need(ending == bytes(512), "archive_end")
            position += 1024
            while position < identity[2]:
                tail = os.pread(fd, min(65536, identity[2] - position), position)
                need(tail and not tail.strip(b"\x00"), "archive_trailing_data")
                position += len(tail)
            return members
        count += 1
        need(count <= MAX_OCI_ENTRIES, "archive_entry_bound")
        expected = sum(header[:148]) + 8 * 32 + sum(header[156:])
        need(archive_octal(header[148:156]) == expected, "archive_header_checksum")
        need(header[257:263] == b"ustar\x00" and header[263:265] == b"00", "archive_header_format")
        name, kind, size = archive_name(header), header[156:157], archive_octal(header[124:136])
        need(not header[157:257].strip(b"\x00"), "archive_link")
        need(archive_octal(header[329:337] or b"0") == 0 and archive_octal(header[337:345] or b"0") == 0, "archive_device")
        if kind == b"x":
            need(pending is None and size <= 65536 and not name.startswith("/") and ".." not in name.split("/"), "archive_pax_header")
            end = position + 512 + ((size + 511) // 512) * 512
            need(end <= identity[2], "archive_truncated")
            need(not os.pread(fd, end - position - 512 - size, position + 512 + size).strip(b"\x00"), "archive_padding")
            pending = archive_pax(os.pread(fd, size, position + 512))
            position = end
            continue
        if pending is not None:
            size = int(pending.get(b"size", str(size).encode()))
            pending = None
        need(name not in members, "archive_duplicate_entry")
        directory = kind == b"5" and name in {"blobs/", "blobs/sha256/"} and size == 0
        blob_path = name.startswith("blobs/sha256/") and lower_sha("sha256:" + name[13:])
        need(directory or (kind in {b"0", b"\x00"} and (blob_path or name in {"index.json", "oci-layout"})), "archive_entry_path_type")
        total += size
        need(total <= MAX_OCI_BLOBS, "archive_blob_bound")
        need(blob_path or size <= MAX_OCI_METADATA, "archive_metadata_bound")
        offset = position + 512
        position = offset + ((size + 511) // 512) * 512
        need(position <= identity[2], "archive_truncated")
        padding = os.pread(fd, (512 - size % 512) % 512, offset + size)
        need(not padding.strip(b"\x00"), "archive_padding")
        members[name] = (offset, size, directory)
    raise GateError("archive_end")

def archive_blobs(fd, identity, members):
    blobs = {}
    for name, (offset, size, directory) in members.items():
        if directory or not name.startswith("blobs/sha256/"):
            continue
        digest = "sha256:" + name[13:]
        blob = VerifiedBlob(digest, size, offset, fd, identity)
        hashed = hashlib.sha256()
        with blob.open() as stream:
            while chunk := stream.read(1024 * 1024):
                hashed.update(chunk)
        need(hashed.hexdigest() == digest[7:], "archive_blob_digest")
        blobs[digest] = blob
    return blobs

def archive_json(raw):
    def reject_constant(_):
        raise GateError("archive_json")
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=duplicate_object, parse_constant=reject_constant)
    except (UnicodeDecodeError, ValueError, RecursionError) as error:
        raise GateError("archive_json") from error
    need(isinstance(value, dict), "archive_json_object")
    return value

def archive_descriptor(value, blobs, used, media=None, archive=None):
    need(isinstance(value, dict) and lower_sha(value.get("digest")), "archive_descriptor")
    need(type(value.get("size")) is int and value["size"] >= 0, "archive_descriptor_size")
    need(isinstance(value.get("mediaType"), str), "archive_descriptor_media")
    annotations = value.get("annotations", {})
    need(isinstance(annotations, dict) and all(isinstance(key, str) and isinstance(item, str) for key, item in annotations.items()), "archive_descriptor_annotations")
    need("urls" not in value, "archive_descriptor_external")
    if "data" in value:
        need(media == EMPTY_MEDIA and value["data"] == "e30=" and value["size"] == 2, "archive_descriptor_data")
        need(value["digest"] == "sha256:" + hashlib.sha256(b"{}").hexdigest(), "archive_descriptor_data")
        if value["digest"] not in blobs:
            need(archive is not None, "archive_descriptor_binding")
            fd, identity = archive
            blobs[value["digest"]] = VerifiedBlob(value["digest"], 2, -1, fd, identity, b"{}")
    if media is not None:
        need(value["mediaType"] == media and (media != EMPTY_MEDIA or (value["size"] == 2 and value["digest"] == "sha256:" + hashlib.sha256(b"{}").hexdigest())), "archive_descriptor_media")
    blob = blobs.get(value["digest"])
    need(blob is not None and blob.size == value["size"], "archive_descriptor_binding")
    used.add(blob.digest)
    return blob

def archive_manifest(descriptor, blobs, used, subject=None):
    blob = archive_descriptor(descriptor, blobs, used, MANIFEST_MEDIA)
    document = archive_json(blob.metadata())
    need(document.get("schemaVersion") == 2 and type(document.get("schemaVersion")) is int, "archive_manifest_schema")
    need(document.get("mediaType") == MANIFEST_MEDIA, "archive_manifest_media")
    if subject is None:
        need("subject" not in document and "artifactType" not in document, "archive_manifest_shape")
    else:
        need(document.get("artifactType") == ATTESTATION_ARTIFACT, "archive_attestation_artifact")
        attached = document.get("subject")
        need(isinstance(attached, dict) and all(attached.get(key) == subject.get(key) for key in ("mediaType", "digest", "size")), "archive_attestation_subject_descriptor")
        archive_descriptor(attached, blobs, used, MANIFEST_MEDIA)
    config = archive_descriptor(document.get("config"), blobs, used, CONFIG_MEDIA if subject is None else EMPTY_MEDIA, (blob.fd, blob.identity))
    layers = document.get("layers")
    need(isinstance(layers, list), "archive_manifest_layers")
    return blob, archive_json(config.metadata()), layers

def archive_platform(descriptor, arch):
    platform = descriptor.get("platform")
    need(isinstance(platform, dict), "archive_platform")
    need(set(platform).issubset({"os", "architecture", "variant"}), "archive_platform_shape")
    need(platform.get("os") == "linux" and platform.get("architecture") == arch, "archive_platform")
    need("variant" not in platform or (arch == "arm64" and platform["variant"] == "v8"), "archive_platform_variant")

def archive_image(descriptor, blobs, used, arch, version, sha, source_url):
    archive_platform(descriptor, arch)
    blob, config, layers = archive_manifest(descriptor, blobs, used)
    need(config.get("os") == "linux" and config.get("architecture") == arch, "archive_config_platform")
    need("variant" not in config or (arch == "arm64" and config["variant"] == "v8"), "archive_config_variant")
    options = config.get("config")
    labels = options.get("Labels") if isinstance(options, dict) else None
    expected = {"org.opencontainers.image.version": version, "org.opencontainers.image.revision": sha,
                "org.opencontainers.image.source": source_url}
    need(isinstance(labels, dict) and all(labels.get(key) == value for key, value in expected.items()), "archive_config_labels")
    allowed = {"application/vnd.oci.image.layer.v1.tar", "application/vnd.oci.image.layer.v1.tar+gzip", "application/vnd.oci.image.layer.v1.tar+zstd"}
    for layer in layers:
        archive_descriptor(layer, blobs, used)
        need(layer["mediaType"] in allowed, "archive_layer_media")
    return blob

def archive_predicate(statement, kind, sha, source_url, arch):
    predicate = statement.get("predicate")
    need(isinstance(predicate, dict) and predicate, "archive_attestation_predicate")
    if kind == SPDX_TYPE:
        need(predicate.get("SPDXID") == "SPDXRef-DOCUMENT", "archive_sbom_document")
        spdx_version = predicate.get("spdxVersion")
        need(isinstance(spdx_version, str) and re.fullmatch(r"SPDX-[0-9]+\.[0-9]+", spdx_version) is not None, "archive_sbom_version")
        need(isinstance(predicate.get("creationInfo"), dict), "archive_sbom_creation")
        return
    if kind == "https://slsa.dev/provenance/v1":
        definition, details = predicate.get("buildDefinition"), predicate.get("runDetails")
        need(isinstance(definition, dict) and isinstance(details, dict), "archive_provenance_shape")
        build_type = definition.get("buildType")
        need(isinstance(build_type, str) and build_type in BUILD_TYPES and isinstance(details.get("builder"), dict), "archive_provenance_builder")
        metadata = details.get("metadata", {})
        vcs_metadata = metadata.get("buildkit_metadata", {}) if isinstance(metadata, dict) else None
    else:
        build_type = predicate.get("buildType")
        need(isinstance(build_type, str) and build_type in BUILD_TYPES and isinstance(predicate.get("builder"), dict), "archive_provenance_builder")
        invocation = predicate.get("invocation")
        need(isinstance(invocation, dict), "archive_provenance_invocation")
        environment = invocation.get("environment", {})
        need(isinstance(environment, dict) and environment.get("platform", "linux/" + arch) == "linux/" + arch, "archive_provenance_builder_platform")
        metadata = predicate.get("metadata", {})
        vcs_metadata = metadata.get("https://mobyproject.org/buildkit@v1#metadata", {}) if isinstance(metadata, dict) else None
    need(isinstance(vcs_metadata, dict), "archive_provenance_metadata")
    vcs = vcs_metadata.get("vcs", {})
    need(isinstance(vcs, dict), "archive_provenance_vcs")
    need(vcs.get("revision") == sha and vcs.get("source") in (source_url, source_url + ".git"), "archive_provenance_source")

def archive_attestation(descriptor, blobs, used, subject_descriptor, sha, source_url, arch):
    subject = subject_descriptor["digest"]
    annotations = descriptor.get("annotations")
    need(isinstance(annotations, dict) and annotations.get("vnd.docker.reference.type") == "attestation-manifest", "archive_attestation_descriptor")
    subjects = [annotations[key] for key in ("vnd.docker.reference.digest", "com.docker.reference.digest") if key in annotations]
    need(subjects and all(item == subject for item in subjects), "archive_attestation_subject")
    need(descriptor.get("platform") == {"os": "unknown", "architecture": "unknown"}, "archive_attestation_platform")
    blob, config, layers = archive_manifest(descriptor, blobs, used, subject_descriptor)
    need(config == {} and blobs["sha256:" + hashlib.sha256(b"{}").hexdigest()].metadata() == b"{}", "archive_attestation_config")
    kinds = []
    for layer in layers:
        payload = archive_descriptor(layer, blobs, used, STATEMENT_MEDIA)
        statement = archive_json(payload.metadata())
        need(statement.get("_type") in {"https://in-toto.io/Statement/v0.1", "https://in-toto.io/Statement/v1"}, "archive_attestation_type")
        kind = statement.get("predicateType")
        need(isinstance(kind, str) and kind in PROVENANCE_TYPES | {SPDX_TYPE}, "archive_attestation_kind")
        need(layer.get("annotations", {}).get("in-toto.io/predicate-type") == kind, "archive_attestation_annotation")
        subjects = statement.get("subject")
        need(isinstance(subjects, list) and subjects, "archive_attestation_statement_subject")
        for item in subjects:
            need(isinstance(item, dict) and isinstance(item.get("name"), str) and item["name"], "archive_attestation_statement_subject")
            need(item.get("digest") == {"sha256": subject[7:]}, "archive_attestation_statement_subject")
        archive_predicate(statement, kind, sha, source_url, arch)
        kinds.append("provenance" if kind in PROVENANCE_TYPES else "sbom")
    need(kinds, "archive_attestation_coverage")
    return blob, kinds

def archive_root(fd, identity, members, blobs, expected_digest):
    used = set()
    for name in ("oci-layout", "index.json"):
        need(name in members and not members[name][2], "archive_layout_missing")
    def document(name):
        offset, size, _ = members[name]
        return archive_json(os.pread(fd, size, offset))
    need(document("oci-layout") == {"imageLayoutVersion": "1.0.0"}, "archive_layout_version")
    layout = document("index.json")
    need(type(layout.get("schemaVersion")) is int and layout["schemaVersion"] == 2, "archive_layout_schema")
    need(layout.get("mediaType", INDEX_MEDIA) == INDEX_MEDIA, "archive_layout_media")
    roots = layout.get("manifests")
    need(isinstance(roots, list) and len(roots) == 1, "archive_layout_roots")
    root = archive_descriptor(roots[0], blobs, used, INDEX_MEDIA)
    need(root.digest == expected_digest, "archive_root_digest")
    index = archive_json(root.metadata())
    need(type(index.get("schemaVersion")) is int and index["schemaVersion"] == 2 and index.get("mediaType") == INDEX_MEDIA, "archive_index_schema")
    children = index.get("manifests")
    need(isinstance(children, list) and 1 <= len(children) <= MAX_OCI_ENTRIES, "archive_index_children")
    need(all(isinstance(item, dict) for item in children), "archive_index_children")
    return root, children, used


def archive_graph(children, blobs, used):
    leaves, order, visited = [], [], set()

    def visit(descriptor, depth):
        need(depth <= 32, "archive_index_depth")
        blob = archive_descriptor(descriptor, blobs, used)
        need(blob.digest not in visited, "archive_manifest_duplicate")
        visited.add(blob.digest)
        if descriptor["mediaType"] == MANIFEST_MEDIA:
            leaves.append(descriptor)
        else:
            need(descriptor["mediaType"] == INDEX_MEDIA, "archive_index_descriptor")
            index = archive_json(blob.metadata())
            need(type(index.get("schemaVersion")) is int and index["schemaVersion"] == 2 and index.get("mediaType") == INDEX_MEDIA, "archive_index_schema")
            nested = index.get("manifests")
            need(isinstance(nested, list) and nested, "archive_index_children")
            for child in nested:
                visit(child, depth + 1)
        order.append(blob)
    for descriptor in children:
        visit(descriptor, 1)
    return leaves, order


def validate_archive(path, expected_digest, image_id, image_name, version, source_sha, arch, source_url):
    need(lower_sha(expected_digest) and isinstance(image_id, str) and safe_id(image_id) and isinstance(arch, str) and safe_arch(arch), "archive_identity")
    need(isinstance(version, str) and oci_version(version) and isinstance(source_sha, str) and re.fullmatch(r"[0-9a-f]{40}", source_sha), "archive_identity")
    need(isinstance(image_name, str) and image_name and isinstance(source_url, str) and source_url.startswith("https://github.com/"), "archive_identity")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        identity = archive_identity(fd)
        members = archive_members(fd, identity)
        blobs = archive_blobs(fd, identity, members)
        root, children, used = archive_root(fd, identity, members, blobs, expected_digest)
        leaves, order = archive_graph(children, blobs, used)
        runnable = [item for item in leaves if item.get("annotations", {}).get("vnd.docker.reference.type") != "attestation-manifest"]
        attestations = [item for item in leaves if item not in runnable]
        need(len(runnable) == 1 and attestations, "archive_platform_set")
        image = archive_image(runnable[0], blobs, used, arch, version, source_sha, source_url)
        kinds = []
        for descriptor in attestations:
            _, covered = archive_attestation(descriptor, blobs, used, runnable[0], source_sha, source_url, arch)
            kinds.extend(covered)
        need(sorted(kinds) == ["provenance", "sbom"], "archive_attestation_coverage")
        need(used == set(blobs), "archive_blob_closure")
        need(archive_identity(fd) == identity, "archive_changed")
        return VerifiedArchive(root.digest, image_id, image_name, version, source_sha, arch,
                               MappingProxyType(blobs), (root, *order), image.digest, fd)
    except BaseException:
        os.close(fd)
        raise
