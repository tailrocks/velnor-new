#!/usr/bin/env python3
"""Write the deterministic source manifest for a validated saved image."""

import hashlib
import json
import os
import pathlib
import sys
import tarfile

from archive_identity import (
    ARCHIVE_NAME,
    CERTIFICATE_IDENTITY,
    DEFAULT_ENV,
    ENTRYPOINT,
    IMAGE_TAG,
    IMAGE_USER,
    REPOSITORY,
    SIGNER_REF,
    SIGNER_WORKFLOW,
    SOURCE_SHA,
    archive_identity,
    fail,
    parse_json,
    require_image,
)


def manifest_bytes(source_sha, identity, archive_sha):
    document = {
        "schema_version": 1,
        "repository": REPOSITORY,
        "source_ref": "refs/heads/main",
        "source_commit": source_sha,
        "signer_workflow": SIGNER_WORKFLOW,
        "signer_ref": SIGNER_REF,
        "workflow_authority_sha": os.environ["VELNOR_WORKFLOW_AUTHORITY_SHA"],
        "trusted_signing_identity": {
            "oidc_issuer": "https://token.actions.githubusercontent.com",
            "certificate_identity": CERTIFICATE_IDENTITY,
        },
        "platform": "linux/amd64",
        "archive_format": identity["archive_format"],
        "oci_index_sha256": identity["oci_index_sha256"],
        "image_manifest_digest": identity["image_manifest_digest"],
        "config_digest": identity["config_digest"],
        "archive_name": ARCHIVE_NAME,
        "archive_sha256": archive_sha,
        "protocol_version": 1,
        "image_user": IMAGE_USER,
        "entrypoint": ENTRYPOINT[0],
    }
    return (json.dumps(document, separators=(",", ":"), ensure_ascii=True) + "\n").encode()


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: write_manifest.py INSPECT_JSON ARCHIVE OUTPUT")
    source_sha = os.environ.get("VELNOR_SOURCE_SHA", "")
    authority_sha = os.environ.get("VELNOR_WORKFLOW_AUTHORITY_SHA", "")
    if not SOURCE_SHA.fullmatch(authority_sha):
        fail("workflow authority is not a full lowercase SHA")
    if os.environ.get("GITHUB_REPOSITORY") != REPOSITORY or os.environ.get("GITHUB_REF") != SIGNER_REF:
        fail("release source is not the expected repository main ref")
    if os.environ.get("GITHUB_SHA") != source_sha:
        fail("release source SHA differs from the checked-out workflow SHA")
    image_document = parse_json(pathlib.Path(sys.argv[1]).read_bytes(), "docker inspect output")
    if type(image_document) is not list or len(image_document) != 1:
        fail("docker image inspect returned an unexpected image count")
    archive_path = pathlib.Path(sys.argv[2])
    identity = archive_identity(archive_path)
    require_image(image_document[0], source_sha, identity)
    digest = hashlib.sha256()
    with archive_path.open("rb") as archive_file:
        for chunk in iter(lambda: archive_file.read(1024 * 1024), b""):
            digest.update(chunk)
    pathlib.Path(sys.argv[3]).write_bytes(
        manifest_bytes(source_sha, identity, digest.hexdigest())
    )


if __name__ == "__main__":
    try:
        main()
    except (OSError, KeyError, ValueError, json.JSONDecodeError, tarfile.TarError) as error:
        print(f"resource-probe manifest creation failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
