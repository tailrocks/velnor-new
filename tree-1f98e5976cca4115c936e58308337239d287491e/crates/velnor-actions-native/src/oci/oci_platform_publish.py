"""Publish exact locally built OCI bytes after current full CI admission."""

import os

from oci_archive import validate_archive
from oci_digest import (
    lower_sha,
    need,
    oci_version,
    publish_admission_main,
    repo_config,
    required,
    safe_arch,
    safe_id,
    source_sha,
)
from oci_registry import publish_verified


def identity(config):
    repository = repo_config(config)
    need(config.get("ci_workflow") in {"ci.yml", ".github/workflows/ci.yml"}, "publisher_ci")
    image, image_id = required("IMAGE"), required("IMAGE_ID")
    arch, version, sha = required("ARCH"), required("VERSION"), required("SOURCE_SHA")
    digest, path, docker_config = required("DIGEST"), required("OCI_ARCHIVE"), required("DOCKER_CONFIG")
    need(safe_id(image_id) and safe_arch(arch) and oci_version(version), "publisher_identity")
    need(source_sha(sha) and lower_sha(digest), "publisher_digest_source")
    need(required("REF") == "refs/tags/v" + version, "publisher_version_ref")
    need(os.path.isabs(docker_config) and os.path.normpath(docker_config) == docker_config, "publisher_config_path")
    need(docker_config.endswith("/velnor/oci-docker"), "publisher_config_path")
    expected = os.path.join(os.path.dirname(docker_config), f"oci-{image_id}-{arch}.tar")
    need(path == expected, "publisher_archive_path")
    return path, digest, image_id, image, version, sha, arch, "https://github.com/" + repository, docker_config


def main(config):
    path, digest, image_id, image, version, sha, arch, source_url, docker_config = identity(config)
    with validate_archive(path, digest, image_id, image, version, sha, arch, source_url) as archive:
        def admission():
            publish_admission_main(config)
        published = publish_verified(archive, image, docker_config, admission, admission)
        need(published == digest, "publisher_result_digest")
