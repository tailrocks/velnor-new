"""Validate supported image archives and their inspected Docker identity."""

import hashlib
import json
import pathlib
import re
import tarfile


REPOSITORY = "tailrocks/velnor-new"
SIGNER_WORKFLOW = ".github/workflows/product-release-images.yml"
SIGNER_REF = "refs/heads/main"
CERTIFICATE_IDENTITY = (
    "https://github.com/tailrocks/velnor-new/"
    ".github/workflows/product-release-images.yml@refs/heads/main"
)
ARCHIVE_FORMAT = "buildkit-oci-layout-docker-compat-v1"
ARCHIVE_NAME = "velnor-resource-probe-linux-amd64.tar"
IMAGE_TAG = "velnor-resource-probe:linux-amd64"
IMAGE_USER = "65532:65532"
ENTRYPOINT = ["/velnor/resource-probe"]
DEFAULT_ENV = ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"]
SHA256 = re.compile(r"^sha256:[0-9a-f]{64}$")
SOURCE_SHA = re.compile(r"^[0-9a-f]{40}$")
BLOB_PATH = re.compile(r"^blobs/sha256/([0-9a-f]{64})$")
IMAGE_MEDIA_TYPES = {
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
}
CONFIG_MEDIA_TYPES = {
    "application/vnd.oci.image.config.v1+json",
    "application/vnd.docker.container.image.v1+json",
}
LAYER_MEDIA_TYPES = {
    "application/vnd.oci.image.layer.v1.tar+gzip",
    "application/vnd.docker.image.rootfs.diff.tar.gzip",
}
MAX_ARCHIVE_BYTES = 16 * 1024 * 1024
MAX_ARCHIVE_MEMBERS = 256
MAX_MEMBER_BYTES = 8 * 1024 * 1024
MAX_MANIFEST_BYTES = 64 * 1024


def fail(message):
    raise ValueError(message)


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            fail("archive JSON contains duplicate keys")
        value[key] = item
    return value


def parse_json(data, label):
    try:
        return json.loads(data, object_pairs_hook=unique_object)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"{label} is invalid JSON: {error}")


def digest_value(value, label):
    if type(value) is not str or not SHA256.fullmatch(value):
        fail(f"{label} is not a full SHA-256 digest")
    return value


def blob_path(value, label):
    if type(value) is not str:
        fail(f"{label} is not a blob path")
    match = BLOB_PATH.fullmatch(value)
    if match is None:
        fail(f"{label} is an unsafe blob path")
    return "sha256:" + match.group(1)


def read_archive(archive_path):
    size = archive_path.stat().st_size
    if size <= 0 or size > MAX_ARCHIVE_BYTES:
        fail("saved archive is empty or exceeds its size cap")
    files = {}
    directories = set()
    names = set()
    total = 0
    with tarfile.open(archive_path, "r:") as archive:
        for count, member in enumerate(archive, start=1):
            if count > MAX_ARCHIVE_MEMBERS:
                fail("saved archive has too many entries")
            name = member.name
            if name in names:
                fail("saved archive contains duplicate paths")
            names.add(name)
            if member.isdir():
                if name not in {"blobs", "blobs/sha256"} or member.size != 0:
                    fail("saved archive contains an unrecognized directory")
                directories.add(name)
                continue
            if not member.isfile() or member.size < 0 or member.size > MAX_MEMBER_BYTES:
                fail("saved archive contains a non-regular or oversized entry")
            if name not in {"oci-layout", "index.json", "manifest.json"} and not BLOB_PATH.fullmatch(name):
                fail("saved archive contains an unrecognized or unsafe path")
            total += member.size
            if total > MAX_ARCHIVE_BYTES:
                fail("saved archive expands beyond its size cap")
            stream = archive.extractfile(member)
            if stream is None:
                fail("saved archive entry cannot be read")
            contents = stream.read(member.size + 1)
            if len(contents) != member.size:
                fail("saved archive entry size does not match its header")
            if name.startswith("blobs/"):
                expected = blob_path(name, "blob name")
                if "sha256:" + hashlib.sha256(contents).hexdigest() != expected:
                    fail("saved archive blob name does not match its bytes")
            files[name] = contents
    required_files = {"oci-layout", "index.json", "manifest.json"}
    if not required_files.issubset(files) or directories != {"blobs", "blobs/sha256"}:
        fail("saved archive is not the supported OCI layout")
    return files


def require_descriptor(descriptor, files, referenced, label):
    if type(descriptor) is not dict:
        fail(f"{label} descriptor is malformed")
    digest = digest_value(descriptor.get("digest"), f"{label} digest")
    size = descriptor.get("size")
    if type(size) is not int or size < 0:
        fail(f"{label} size is invalid")
    path = "blobs/sha256/" + digest.removeprefix("sha256:")
    contents = files.get(path)
    if contents is None or len(contents) != size:
        fail(f"{label} descriptor does not match a saved blob")
    referenced.add(path)
    return digest, contents


def require_blob(files, digest, expected_size, referenced, label):
    digest_value(digest, f"{label} digest")
    if type(expected_size) is not int or expected_size < 0:
        fail(f"{label} size is invalid")
    path = "blobs/sha256/" + digest.removeprefix("sha256:")
    contents = files.get(path)
    if contents is None or len(contents) != expected_size:
        fail(f"{label} does not match a saved blob")
    referenced.add(path)
    return contents


def compatibility_entry(files):
    if len(files["manifest.json"]) > MAX_MANIFEST_BYTES:
        fail("Docker compatibility manifest exceeds its size cap")
    document = parse_json(files["manifest.json"], "Docker compatibility manifest")
    if type(document) is not list or len(document) != 1:
        fail("archive must contain exactly one Docker image record")
    entry = document[0]
    if type(entry) is not dict or entry.get("RepoTags") != [IMAGE_TAG]:
        fail("archive does not contain the exact resource-probe tag")
    config_digest = blob_path(entry.get("Config"), "Docker config path")
    layers = entry.get("Layers")
    if type(layers) is not list:
        fail("Docker compatibility layer list is invalid")
    layer_digests = [blob_path(layer, "Docker layer path") for layer in layers]
    return config_digest, layer_digests


def image_manifest(descriptor, files, compatible_config, compatible_layers, referenced):
    media_type = descriptor.get("mediaType")
    digest, contents = require_descriptor(descriptor, files, referenced, "image")
    manifest = parse_json(contents, "image manifest")
    if type(manifest) is not dict or manifest.get("schemaVersion") != 2:
        fail("image manifest schema is unsupported")
    if media_type not in IMAGE_MEDIA_TYPES or manifest.get("mediaType") != media_type:
        fail("image manifest media type is unsupported or inconsistent")
    config = manifest.get("config")
    if type(config) is not dict:
        fail("image manifest config descriptor is missing")
    config_digest = digest_value(config.get("digest"), "image config digest")
    if config_digest != compatible_config:
        fail("image manifest config differs from Docker compatibility metadata")
    if config.get("mediaType") not in CONFIG_MEDIA_TYPES:
        fail("image config media type is unsupported")
    config_bytes = require_blob(
        files, config_digest, config.get("size"), referenced, "image config"
    )
    config_document = parse_json(config_bytes, "image config")
    if type(config_document) is not dict:
        fail("image config is not an object")
    if config_document.get("os") != "linux" or config_document.get("architecture") != "amd64":
        fail("saved image config is not linux/amd64")
    layers = manifest.get("layers")
    if type(layers) is not list:
        fail("image manifest layer list is invalid")
    layer_digests = []
    for layer in layers:
        if type(layer) is not dict:
            fail("image layer descriptor is malformed")
        if layer.get("mediaType") not in LAYER_MEDIA_TYPES:
            fail("image layer media type is unsupported")
        layer_digest = digest_value(layer.get("digest"), "image layer digest")
        require_blob(files, layer_digest, layer.get("size"), referenced, "image layer")
        layer_digests.append(layer_digest)
    if layer_digests != compatible_layers:
        fail("image manifest layers differ from Docker compatibility metadata")
    platform = descriptor.get("platform")
    if platform is not None and (
        type(platform) is not dict
        or set(platform) != {"os", "architecture"}
        or platform != {"os": "linux", "architecture": "amd64"}
    ):
        fail("image descriptor platform is not exactly linux/amd64")
    annotations = descriptor.get("annotations", {})
    if type(annotations) is not dict:
        fail("image descriptor annotations are malformed")
    if annotations.get("config.digest") not in (None, config_digest):
        fail("image descriptor config annotation is inconsistent")
    return {
        "digest": digest,
        "media_type": media_type,
        "config_digest": config_digest,
        "config_document": config_document,
    }


def validate_referrer(descriptor, files, image_digest, referenced):
    if "platform" in descriptor:
        fail("archive referrer must not be a platform image descriptor")
    if descriptor.get("mediaType") != "application/vnd.oci.image.manifest.v1+json":
        fail("archive referrer descriptor media type is unsupported")
    annotations = descriptor.get("annotations")
    if type(annotations) is not dict or annotations.get("io.containerd.manifest.subject") != image_digest:
        fail("archive referrer does not name the selected image subject")
    _, contents = require_descriptor(descriptor, files, referenced, "referrer")
    manifest = parse_json(contents, "referrer manifest")
    if type(manifest) is not dict or manifest.get("mediaType") != "application/vnd.oci.image.manifest.v1+json":
        fail("archive referrer manifest type is unsupported")
    config = manifest.get("config")
    if type(config) is not dict:
        fail("archive referrer config descriptor is malformed")
    config_digest = digest_value(config.get("digest"), "referrer config digest")
    config_bytes = require_blob(
        files, config_digest, config.get("size"), referenced, "referrer config"
    )
    parse_json(config_bytes, "referrer config")
    layers = manifest.get("layers")
    if type(layers) is not list or not layers:
        fail("archive referrer layers are missing")
    subject_found = False
    for layer in layers:
        if type(layer) is not dict:
            fail("archive referrer layer descriptor is malformed")
        media_type = layer.get("mediaType")
        layer_digest = digest_value(layer.get("digest"), "referrer layer digest")
        layer_bytes = require_blob(
            files, layer_digest, layer.get("size"), referenced, "referrer layer"
        )
        if media_type == "application/vnd.in-toto+json":
            statement = parse_json(layer_bytes, "in-toto statement")
            subjects = statement.get("subject") if type(statement) is dict else None
            if type(subjects) is not list:
                fail("in-toto referrer has no subject list")
            subject_found |= any(
                type(item) is dict
                and type(item.get("digest")) is dict
                and item["digest"].get("sha256") == image_digest.removeprefix("sha256:")
                for item in subjects
            )
        else:
            fail("archive referrer layer type is unsupported")
    if not subject_found:
        fail("in-toto referrer does not bind to the selected image")


def archive_identity(archive_path):
    files = read_archive(archive_path)
    layout = parse_json(files["oci-layout"], "OCI layout")
    if layout != {"imageLayoutVersion": "1.0.0"}:
        fail("OCI layout version is unsupported")
    index_bytes = files["index.json"]
    index = parse_json(index_bytes, "OCI index")
    if type(index) is not dict or index.get("schemaVersion") != 2:
        fail("OCI index schema is unsupported")
    if index.get("mediaType") != "application/vnd.oci.image.index.v1+json":
        fail("OCI index media type is unsupported")
    compatible_config, compatible_layers = compatibility_entry(files)
    descriptors = index.get("manifests")
    if type(descriptors) is not list or not descriptors:
        fail("OCI index has no descriptors")
    referenced = set()
    image_candidates = []
    referrers = []
    for descriptor in descriptors:
        if type(descriptor) is not dict:
            fail("OCI index descriptor is malformed")
        annotations = descriptor.get("annotations", {})
        if type(annotations) is not dict:
            fail("OCI index annotations are malformed")
        if "io.containerd.manifest.subject" in annotations:
            referrers.append(descriptor)
            continue
        if descriptor.get("mediaType") not in IMAGE_MEDIA_TYPES:
            fail("OCI index contains an unsupported non-image descriptor")
        candidate = image_manifest(
            descriptor, files, compatible_config, compatible_layers, referenced
        )
        image_candidates.append(candidate)
    if len(image_candidates) != 1:
        fail("OCI index must resolve to exactly one linux/amd64 image")
    if len(referrers) > 1:
        fail("OCI index contains multiple provenance referrers")
    selected = image_candidates[0]
    for referrer in referrers:
        validate_referrer(referrer, files, selected["digest"], referenced)
    archive_blobs = {name for name in files if name.startswith("blobs/")}
    if archive_blobs != referenced:
        fail("archive contains missing or unreferenced blobs")
    return {
        "archive_format": ARCHIVE_FORMAT,
        "oci_index_sha256": hashlib.sha256(index_bytes).hexdigest(),
        "image_manifest_digest": selected["digest"],
        "image_manifest_media_type": selected["media_type"],
        "config_digest": selected["config_digest"],
        "config_document": selected["config_document"],
    }


def require_image(image, source_sha, identity):
    if type(image) is not dict or image.get("Os") != "linux" or image.get("Architecture") != "amd64":
        fail("loaded image is not linux/amd64")
    if not SOURCE_SHA.fullmatch(source_sha):
        fail("source commit is not a full lowercase SHA")
    image_id = digest_value(image.get("Id"), "loaded image ID")
    config = image.get("Config")
    if type(config) is not dict:
        fail("loaded image config is missing")
    config_document = identity["config_document"].get("config")
    if type(config_document) is not dict:
        fail("saved image config settings are missing")
    if config.get("User") != IMAGE_USER or config.get("Entrypoint") != ENTRYPOINT:
        fail("loaded image user or entrypoint does not match the fixed profile")
    if config.get("Cmd") not in (None, []) or config.get("Env") != DEFAULT_ENV:
        fail("loaded image command or environment does not match the fixed profile")
    labels = config.get("Labels")
    if type(labels) is not dict or labels.get("org.opencontainers.image.revision") != source_sha:
        fail("loaded image revision label differs from the exact source commit")
    for key in ("User", "Entrypoint", "Cmd", "Env", "Labels"):
        if config.get(key) != config_document.get(key):
            fail("loaded image config differs from the saved image config")
    descriptor = image.get("Descriptor")
    if "Descriptor" not in image:
        if image_id != identity["config_digest"]:
            fail("classic image ID differs from the saved config digest")
    elif (
        type(descriptor) is not dict
        or descriptor.get("mediaType") != identity["image_manifest_media_type"]
        or descriptor.get("digest") != identity["image_manifest_digest"]
        or image_id != identity["image_manifest_digest"]
    ):
        fail("containerd image ID differs from the selected manifest digest")
