# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
from contextlib import ExitStack
import json
from pathlib import Path
from tempfile import TemporaryDirectory

import job_image_authority_publish as publisher
import job_image_authority_registry as registry
import runner_image_authority_candidate as candidate
import runner_image_authority_custody as custody
import runner_image_authority_identity as identity

PROFILES = {
    "runner": registry.ImageProfile.RUNNER,
    "dind": registry.ImageProfile.DIND,
    "resource_probe": registry.ImageProfile.RESOURCE_PROBE,
}


class AuthorityError(ValueError):
    pass


def _current(source, current_source):
    if current_source() != source:
        raise AuthorityError("Publication source is no longer current main.")


def _family_identity(admitted):
    first = admitted["runner"]["record"]
    probe = first["provenance"]["named_contexts"].get("resource_probe")
    if probe is None:
        raise AuthorityError("Qualified resource-probe context is missing.")
    for item in admitted.values():
        record = item["record"]
        if (
            record["source"]["inputs"] != first["source"]["inputs"]
            or record["lock"] != first["lock"]
            or record["provenance"]["named_contexts"].get("resource_probe") != probe
        ):
            raise AuthorityError("Qualified runner-family source identities differ.")


def _manifest(source, receipt, admitted, published):
    lock = admitted["runner"]["record"]["lock"]
    images = {}
    for role, profile in PROFILES.items():
        graph = admitted[role]["graph"]
        digest = graph["platform_manifest_digest"]
        if published[role]["manifest"]["digest"] != digest:
            raise AuthorityError("Published manifest differs from qualified graph.")
        images[role] = {
            "repository": f"ghcr.io/{profile.value}", "tag": f"sha-{source}",
            "registry_digest": digest, "platform_manifest_digest": digest,
            "config_digest": graph["image_configuration_digest"],
            "build_source": admitted[role]["build_source"],
        }
    return {
        "schema_version": 2, "repository": identity.REPOSITORY,
        "source_ref": "refs/heads/main", "source_commit": source,
        "runner_lock_sha256": receipt["runner_lock_sha256"],
        "signer_workflow": ".github/workflows/product-release-images.yml",
        "signer_ref": "refs/heads/main", "workflow_authority_sha": source,
        "trusted_signing_identity": {
            "oidc_issuer": "https://token.actions.githubusercontent.com",
            "certificate_identity": f"{identity.REPOSITORY_URI}/.github/workflows/product-release-images.yml@refs/heads/main",
        },
        "platform": "linux/amd64",
        "versions": {name: lock[f"{name}_version"] for name in ["runner", "docker", "buildx", "compose", "node"]},
        "images": images,
        "resource_probe": {"protocol_version": 1, "image_user": "65532:65532", "entrypoint": "/velnor/resource-probe", "working_directory": "/"},
    }


def publish_family(inputs, receipt_raw, verification_output, source, producer_id, producer_attempt,
                   qualifier_id, qualifier_attempt, job_id, lock_raw, private_parent,
                   actor, password, transports, current_source):
    if not isinstance(inputs, dict) or set(inputs) != set(PROFILES) or set(transports) != set(PROFILES):
        raise AuthorityError("Runner-family publication requires exactly three fixed roles.")
    receipt = identity.authenticate_qualification_identity(
        receipt_raw, verification_output, source, producer_id, producer_attempt,
        qualifier_id, qualifier_attempt, job_id,
    )
    admitted = {}
    paths = {}
    published = {}
    with ExitStack() as held:
        for role in PROFILES:
            item = inputs[role]
            paths[role] = held.enter_context(custody.snapshot_artifact(
                item["archive"], receipt["images"][role]["archive"], private_parent,
            ))
            admitted[role] = candidate.verify_candidate(
                paths[role], item["record"], item["envelope"], receipt, role,
                source, producer_id, producer_attempt, lock_raw,
            )
        _family_identity(admitted)
        for role, profile in PROFILES.items():
            _current(source, current_source)
            policy = registry.RegistryPolicy(profile)
            transport = transports[role]
            token = publisher.fetch_registry_token(policy, transport, actor, password)
            graph = admitted[role]["graph"]
            manifest = publisher.read_manifest_bytes(paths[role], graph["platform_manifest_digest"], graph["platform_manifest_size_bytes"])
            with TemporaryDirectory(prefix="runner-blobs-", dir=private_parent) as directory:
                blobs = {digest: publisher.extract_blob(paths[role], digest, size, Path(directory))
                         for digest, size in publisher.blob_descriptors(identity.parse_document(manifest, publisher.MAX_MANIFEST_BYTES)).items()}
                published[role] = publisher.publish_image(policy, transport, token, manifest, blobs)
        for role, profile in PROFILES.items():
            _current(source, current_source)
            policy = registry.RegistryPolicy(profile)
            transport = transports[role]
            token = publisher.fetch_anonymous_pull_token(policy, transport)
            graph = admitted[role]["graph"]
            digest = graph["platform_manifest_digest"]
            manifest = publisher.read_manifest_bytes(paths[role], digest, graph["platform_manifest_size_bytes"])
            publisher.verify_remote_manifest(policy, transport, token, manifest, digest, len(manifest))
            with TemporaryDirectory(prefix="runner-anonymous-blobs-", dir=private_parent) as directory:
                descriptors = publisher.blob_descriptors(identity.parse_document(manifest, publisher.MAX_MANIFEST_BYTES))
                for blob_digest, size in descriptors.items():
                    blob = publisher.extract_blob(paths[role], blob_digest, size, Path(directory))
                    publisher.verify_remote_blob(policy, transport, token, blob_digest, blob, size)
        for role, profile in PROFILES.items():
            _current(source, current_source)
            policy = registry.RegistryPolicy(profile)
            transport = transports[role]
            token = publisher.fetch_registry_token(policy, transport, actor, password)
            graph = admitted[role]["graph"]
            manifest = publisher.read_manifest_bytes(paths[role], graph["platform_manifest_digest"], graph["platform_manifest_size_bytes"])
            publisher.publish_source_tag(policy, transport, token, manifest, source)
        _current(source, current_source)
        return (json.dumps(_manifest(source, receipt, admitted, published), indent=2) + "\n").encode()
