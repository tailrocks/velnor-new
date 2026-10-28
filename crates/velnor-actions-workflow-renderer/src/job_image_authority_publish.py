# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""Fixed image publication assembly, byte verification, and evidence.

Data-only port of the publication orchestration in
``images/job/ubuntu-26.04/candidate/publish.py``. It binds the signed
record to its archive and OCI graph, extracts and verifies blob bytes from
the local archive, publishes every blob and the platform manifest through
an injected registry transport, verifies the remote bytes with HEAD and
GET readback, and builds the signed publication evidence. It never
runs candidate code, never reads process state or a checkout, never opens
a network path itself (the rendered authority step supplies a bounded
stdlib transport). Every registry URL, redirect, and response is
validated through the registry helpers passed in as ``registry`` (the
sibling ``job_image_authority_registry`` module in the rendered body),
so this module keeps its standard-library-only imports.

Transport contract (``transport`` is the rendered bounded HTTPS layer):

* ``transport.request(method, url, headers, body=None, length=0)`` sends one
  control or manifest request and returns a response with ``status``,
  lowercased ``headers``, and buffered ``body`` bytes. ``body`` is either
  ``bytes`` (small control and manifest payloads, never buffered above the
  transport's own small control cap) or a temp-file path object that the
  transport streams from disk in chunks. The returned ``body`` is only ever
  used for small control responses; blob bytes never flow through it.
* ``transport.stream(method, url, headers)`` opens one blob GET and returns a
  stream with ``status``, lowercased ``headers``, ``read(count)``, and
  ``close()``. The caller reads incrementally and always closes. Blob bytes
  are compared chunk by chunk against the extracted temp file, so even a
  1 GiB blob needs only a small constant buffer.

``publish_image`` therefore takes a mapping of descriptor digest to extracted
temp-file path (see ``extract_blob``), never a mapping of digest to bytes.
"""
import hashlib
import json
import re
import tarfile

EVIDENCE_SCHEMA = "velnor.job-image-publication.v1"
PACKAGE = "ghcr.io/tailrocks/velnor-job"
MAX_MANIFEST_BYTES = 2 * 1024 * 1024
MAX_UPLOAD_BYTES = 1024**3
MAX_RESULT_BYTES = 64 * 1024


class AuthorityError(ValueError):
    pass


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise AuthorityError(f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def _reject_constant(value):
    raise AuthorityError(f"JSON constant has an incorrect value: {value}.")


def _json_bytes(data, label):
    try:
        return json.loads(data, object_pairs_hook=_unique_object, parse_constant=_reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise AuthorityError(f"{label} has an incorrect format.") from error


def _sha(data):
    return hashlib.sha256(data).hexdigest()


def check_record_binding(record, archive_digest, archive_size, oci):
    if not isinstance(record, dict):
        raise AuthorityError("Candidate record has an incorrect format.")
    if record.get("archive") != {"sha256": archive_digest, "size_bytes": archive_size}:
        raise AuthorityError("Candidate archive is not the same as the archive in its signed record.")
    if record.get("oci") != oci:
        raise AuthorityError("Candidate OCI graph is not the same as the graph in its signed record.")
    return record


def blob_descriptors(manifest):
    config = manifest.get("config") if isinstance(manifest, dict) else None
    layers = manifest.get("layers") if isinstance(manifest, dict) else None
    if not isinstance(config, dict) or not isinstance(layers, list):
        raise AuthorityError("OCI manifest blob descriptors are incorrect.")
    unique = {}
    for descriptor in [config, *layers]:
        if not isinstance(descriptor, dict):
            raise AuthorityError("OCI blob descriptor is incorrect.")
        digest = descriptor.get("digest")
        size = descriptor.get("size")
        if (
            not isinstance(digest, str)
            or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest)
            or type(size) is not int
            or not 1 <= size <= MAX_UPLOAD_BYTES
        ):
            raise AuthorityError("OCI blob descriptor is incorrect.")
        if digest in unique and unique[digest] != size:
            raise AuthorityError("Repeated OCI blob digest has inconsistent sizes.")
        unique[digest] = size
    return unique


def _archive_blob(archive, digest, limit):
    try:
        with tarfile.open(archive, "r:") as tar:
            member = tar.getmember(f"blobs/sha256/{digest}")
            if not member.isfile() or member.size < 0:
                raise AuthorityError("OCI blob descriptor size is incorrect.")
            stream = tar.extractfile(member)
            if stream is None:
                raise AuthorityError("OCI blob is missing from archive.")
            with stream:
                return stream.read(limit + 1)
    except (OSError, KeyError, tarfile.TarError) as error:
        raise AuthorityError("Cannot read OCI blob from archive.") from error


def read_manifest_bytes(archive, digest, size):
    if type(size) is not int or size > MAX_MANIFEST_BYTES:
        raise AuthorityError("OCI manifest exceeds its size limit.")
    raw = _archive_blob(archive, digest.removeprefix("sha256:"), MAX_MANIFEST_BYTES)
    if len(raw) != size or _sha(raw) != digest.removeprefix("sha256:"):
        raise AuthorityError("OCI platform manifest bytes changed after validation.")
    return raw


def extract_blob(archive, digest, expected_size, directory):
    hexdigest = digest.removeprefix("sha256:")
    if not re.fullmatch(r"[0-9a-f]{64}", hexdigest) or type(expected_size) is not int or expected_size > MAX_UPLOAD_BYTES:
        raise AuthorityError("OCI blob descriptor size is incorrect.")
    output = directory / hexdigest
    hasher = hashlib.sha256()
    size = 0
    try:
        with tarfile.open(archive, "r:") as tar:
            member = tar.getmember(f"blobs/sha256/{hexdigest}")
            if not member.isfile() or member.size != expected_size:
                raise AuthorityError("OCI blob descriptor size is incorrect.")
            source = tar.extractfile(member)
            if source is None:
                raise AuthorityError("OCI blob is missing from archive.")
            with source, output.open("xb") as destination:
                while chunk := source.read(1024 * 1024):
                    size += len(chunk)
                    if size > expected_size:
                        raise AuthorityError("OCI blob exceeds descriptor size.")
                    hasher.update(chunk)
                    destination.write(chunk)
    except (OSError, KeyError, tarfile.TarError) as error:
        raise AuthorityError("Cannot read OCI blob from archive.") from error
    if size != expected_size or hasher.hexdigest() != hexdigest:
        raise AuthorityError("OCI blob bytes are not the same as the bytes identified by their descriptor.")
    return output


def check_record_source(record_source, identity_source):
    """Bind evidence to the validated candidate source; refuse relabeling.

    The candidate bytes are already bound to the record by
    ``check_record_binding``. This binds the record to the authenticated
    identity so unchanged bytes can never be relabeled with another
    source SHA.
    """
    if (
        not isinstance(identity_source, str)
        or not re.fullmatch(r"[0-9a-f]{40}", identity_source)
    ):
        raise AuthorityError("Authenticated source is incorrect.")
    if (
        not isinstance(record_source, dict)
        or record_source.get("commit") != identity_source
    ):
        raise AuthorityError("Candidate source is not the authenticated source.")
    if record_source.get("ref") != "refs/heads/main":
        raise AuthorityError("Candidate source is not the current main source.")
    return identity_source


def _b64encode(data):
    """Encode bytes with the standard Base64 alphabet (no new imports)."""
    alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    out = []
    for index in range(0, len(data), 3):
        chunk = data[index:index + 3]
        unit = int.from_bytes(chunk, "big") << (8 * (3 - len(chunk)))
        for shift in (18, 12, 6, 0):
            out.append(alphabet[(unit >> shift) & 0x3F])
        for position in range(3 - len(chunk)):
            out[-1 - position] = "="
    return "".join(out)


def _request(transport, method, url, headers, body=None, length=0):
    try:
        return transport.request(method, url, headers, body=body, length=length)
    except AuthorityError:
        raise
    except Exception as error:
        raise AuthorityError("Registry request failed or timed out.") from error


def _open_stream(transport, method, url, headers):
    """Open a streaming GET; the caller reads incrementally and closes it."""
    try:
        return transport.stream(method, url, headers)
    except AuthorityError:
        raise
    except Exception as error:
        raise AuthorityError("Registry request failed or timed out.") from error


def _read_exact(reader, count):
    """Read exactly count bytes, looping short reads; short means EOF."""
    chunks = []
    remaining = count
    while remaining:
        chunk = reader.read(min(65536, remaining))
        if not chunk:
            break
        chunks.append(bytes(chunk))
        remaining -= len(chunk)
    return b"".join(chunks)


def fetch_registry_token(registry, transport, actor, password):
    """Exchange actor credentials for a pull/push registry token."""
    if (
        not isinstance(actor, str)
        or not actor
        or any(ord(char) < 32 or ord(char) == 127 for char in actor)
    ):
        raise AuthorityError("Registry actor is incorrect.")
    if not isinstance(password, str) or not password:
        raise AuthorityError("Registry token is missing.")
    challenge = _request(transport, "GET", f"https://{registry.REGISTRY}/v2/", {})
    if challenge.status != 401:
        raise AuthorityError("Registry did not ask for authentication.")
    params = registry.parse_auth_challenge(
        challenge.headers.get("www-authenticate", "")
    )
    url = registry.token_request_url(params)
    basic = _b64encode(f"{actor}:{password}".encode())
    response = _request(
        transport,
        "GET",
        url,
        {"Authorization": f"Basic {basic}", "Accept": "application/json"},
    )
    if response.status != 200:
        raise AuthorityError("Registry token request failed.")
    return registry.parse_token_payload(response.body)


def _authorization(token):
    if not isinstance(token, str) or not token:
        raise AuthorityError("Registry token is missing.")
    return {"Authorization": f"Bearer {token}"}


def head_blob(registry, transport, token, digest, size):
    """Return True when the blob is already present; fail on any other state."""
    headers = _authorization(token)
    headers["Accept"] = "application/octet-stream"
    response = _request(
        transport, "HEAD", registry.registry_url(f"blobs/{digest}"), headers
    )
    return registry.check_head_response(
        response.status, response.headers, digest, size
    )


def _head_manifest_reference(registry, transport, token, reference, digest, size):
    headers = _authorization(token)
    headers["Accept"] = "application/vnd.oci.image.manifest.v1+json"
    response = _request(
        transport, "HEAD", registry.registry_url(f"manifests/{reference}"), headers
    )
    return registry.check_head_response(
        response.status, response.headers, digest, size
    )


def _head_manifest(registry, transport, token, digest, size):
    return _head_manifest_reference(
        registry, transport, token, digest, digest, size
    )


def _open_blob_stream(registry, transport, token, digest):
    """Open the blob GET, following the single permitted download redirect."""
    headers = _authorization(token)
    headers["Accept"] = "application/octet-stream"
    first = _open_stream(transport, "GET", registry.registry_url(f"blobs/{digest}"), headers)
    request_path = f"/v2/{registry.PACKAGE}/blobs/{digest}"
    target = registry.redirect_target(
        request_path, digest, first.status, first.headers.get("location", "")
    )
    if target is None:
        return first
    download_url = f"https://{registry.DOWNLOAD_HOST}{target}"
    try:
        second = _open_stream(transport, "GET", download_url, {"Accept": "application/octet-stream"})
    except AuthorityError:
        first.close()
        raise
    first.close()
    return second


def verify_remote_blob(registry, transport, token, digest, path, size):
    """GET the blob and prove the remote bytes equal the local temp file.

    Streams both sides in lockstep: the registry response is never buffered
    and the local file is compared chunk by chunk, so a 1 GiB blob needs only
    a small constant buffer.
    """
    _require_blob_file(digest, path, size)
    expected_hex = digest.removeprefix("sha256:")
    stream = _open_blob_stream(registry, transport, token, digest)
    try:
        if stream.status != 200:
            raise AuthorityError(f"Registry GET status is incorrect: {stream.status}.")
        if stream.headers.get("content-length") != str(size):
            raise AuthorityError("Registry GET size is different from the specified size.")
        if stream.headers.get("docker-content-digest") not in (None, digest):
            raise AuthorityError("Registry GET digest header is different from the requested digest.")
        hasher = hashlib.sha256()
        remaining = size
        try:
            local = open(path, "rb")
        except OSError as error:
            raise AuthorityError("OCI blob file is missing.") from error
        with local:
            while remaining:
                want = min(65536, remaining)
                remote_chunk = _read_exact(stream, want)
                local_chunk = _read_exact(local, want)
                if len(remote_chunk) != want or remote_chunk != local_chunk:
                    raise AuthorityError("Registry retrieved bytes changed during publication.")
                hasher.update(remote_chunk)
                remaining -= len(remote_chunk)
            if stream.read(1):
                raise AuthorityError("Registry GET size is different from the specified size.")
        if hasher.hexdigest() != expected_hex:
            raise AuthorityError("Registry response bytes have an incorrect digest or size.")
    finally:
        stream.close()
    return {"digest": digest, "size": size, "remote_sha256": expected_hex}


def verify_remote_manifest(registry, transport, token, manifest, digest, size, reference=None):
    """GET the manifest and prove the remote bytes equal the local bytes."""
    if type(size) is not int or not 1 <= size <= MAX_MANIFEST_BYTES:
        raise AuthorityError("OCI manifest exceeds its size limit.")
    if not isinstance(manifest, bytes) or len(manifest) != size:
        raise AuthorityError("OCI manifest exceeds its size limit.")
    if reference is None:
        reference = digest
    if not isinstance(reference, str) or not re.fullmatch(
        r"(?:sha256:[0-9a-f]{64}|sha-[0-9a-f]{40})", reference
    ):
        raise AuthorityError("OCI manifest reference is incorrect.")
    headers = _authorization(token)
    headers["Accept"] = "application/vnd.oci.image.manifest.v1+json"
    stream = _open_stream(transport, "GET", registry.registry_url(f"manifests/{reference}"), headers)
    try:
        registry.redirect_target(
            f"/v2/{registry.PACKAGE}/manifests/{reference}",
            digest,
            stream.status,
            stream.headers.get("location", ""),
        )
        body = bytearray()
        while len(body) <= size:
            chunk = stream.read(min(65536, size + 1 - len(body)))
            if not chunk:
                break
            body.extend(chunk)
        status, response_headers, response_body = stream.status, stream.headers, bytes(body)
    finally:
        stream.close()
    registry.check_get_response(status, response_headers, response_body, digest, size, manifest)
    return {
        "digest": digest,
        "size": size,
        "remote_sha256": digest.removeprefix("sha256:"),
    }


def _require_blob_file(digest, path, size):
    """Require a temp-file handle for a blob; bytes would materialize the image.

    Plain ``open``/``seek`` keeps this module free of ``os`` (the fixed-input
    hermeticity gate pins the import list); the size probe rejects missing,
    truncated, and padded files without reading them.
    """
    if not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
        raise AuthorityError("OCI blob descriptor is incorrect.")
    if isinstance(path, (str, bytes, bytearray, memoryview)):
        raise AuthorityError("OCI blob must be a temporary file path, not bytes.")
    if type(size) is not int or not 1 <= size <= MAX_UPLOAD_BYTES:
        raise AuthorityError("OCI blob descriptor size is incorrect.")
    try:
        stream = open(path, "rb")
    except (OSError, ValueError, TypeError):
        raise AuthorityError("OCI blob file is missing.")
    with stream:
        try:
            stream.seek(0, 2)
            actual = stream.tell()
        except (OSError, ValueError):
            raise AuthorityError("OCI blob file is missing.") from None
    if actual != size:
        raise AuthorityError("OCI blob descriptor size is incorrect.")
    return path


def _begin_upload(registry, transport, token):
    start = registry.registry_url("blobs/uploads/")
    response = _request(
        transport,
        "POST",
        start,
        {**_authorization(token), "Content-Length": "0"},
    )
    if response.status != 202 or "location" not in response.headers:
        raise AuthorityError("Registry upload session could not start.")
    return registry.validate_upload_location(start, response.headers["location"])


def upload_blob(registry, transport, token, digest, path, size):
    """Upload one blob from its temp file, then require HEAD to prove it landed.

    The temp file is streamed from disk through the transport; it is never
    loaded into memory.
    """
    _require_blob_file(digest, path, size)
    location = _begin_upload(registry, transport, token)
    target = registry.upload_target(location, digest)
    try:
        response = _request(
            transport,
            "PUT",
            target,
            {**_authorization(token), "Content-Type": "application/octet-stream"},
            body=path,
            length=size,
        )
    except AuthorityError:
        if head_blob(registry, transport, token, digest, size):
            return
        raise
    if response.status != 201:
        raise AuthorityError("Registry blob upload did not complete.")
    if not head_blob(registry, transport, token, digest, size):
        raise AuthorityError("Registry blob is absent after upload.")


def upload_manifest(registry, transport, token, manifest, digest):
    """Upload the platform manifest, then require HEAD to prove it landed."""
    size = len(manifest)
    url = registry.registry_url(f"manifests/{digest}")
    try:
        response = _request(
            transport,
            "PUT",
            url,
            {
                **_authorization(token),
                "Content-Type": "application/vnd.oci.image.manifest.v1+json",
            },
            body=manifest,
            length=size,
        )
    except AuthorityError:
        if _head_manifest(registry, transport, token, digest, size):
            verify_remote_manifest(registry, transport, token, manifest, digest, size)
            return
        raise
    if response.status != 201:
        raise AuthorityError("Registry manifest upload did not complete.")
    if response.headers.get("docker-content-digest") not in (None, digest):
        raise AuthorityError(
            "Registry response has a manifest digest that is different from the requested digest."
        )
    if not _head_manifest(registry, transport, token, digest, size):
        raise AuthorityError("Registry manifest is absent after upload.")
    verify_remote_manifest(registry, transport, token, manifest, digest, size)


def publish_blob(registry, transport, token, digest, path, size):
    """Upload a blob temp file when absent, then prove the remote bytes with HEAD and GET."""
    _require_blob_file(digest, path, size)
    if not head_blob(registry, transport, token, digest, size):
        upload_blob(registry, transport, token, digest, path, size)
    verify_remote_blob(registry, transport, token, digest, path, size)
    return {
        "digest": digest,
        "size": size,
        "remote_sha256": digest.removeprefix("sha256:"),
    }


def publish_manifest_receipt(registry, transport, token, manifest):
    """Upload the platform manifest when absent, then prove the remote bytes."""
    if (
        not isinstance(manifest, bytes)
        or len(manifest) < 1
        or len(manifest) > MAX_MANIFEST_BYTES
    ):
        raise AuthorityError("OCI manifest exceeds its size limit.")
    digest = "sha256:" + _sha(manifest)
    size = len(manifest)
    if not _head_manifest(registry, transport, token, digest, size):
        upload_manifest(registry, transport, token, manifest, digest)
    verify_remote_manifest(registry, transport, token, manifest, digest, size)
    return {
        "digest": digest,
        "size": size,
        "remote_sha256": digest.removeprefix("sha256:"),
    }


def publish_source_tag(registry, transport, token, manifest, source_sha):
    if not isinstance(source_sha, str) or not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise AuthorityError("Authenticated source is incorrect.")
    digest = "sha256:" + _sha(manifest)
    size = len(manifest)
    tag = f"sha-{source_sha}"
    if _head_manifest_reference(registry, transport, token, tag, digest, size):
        verify_remote_manifest(
            registry, transport, token, manifest, digest, size, reference=tag
        )
        return
    response = _request(
        transport,
        "PUT",
        registry.registry_url(f"manifests/{tag}"),
        {
            **_authorization(token),
            "Content-Type": "application/vnd.oci.image.manifest.v1+json",
        },
        body=manifest,
        length=size,
    )
    if response.status != 201:
        if _head_manifest_reference(registry, transport, token, tag, digest, size):
            verify_remote_manifest(
                registry, transport, token, manifest, digest, size, reference=tag
            )
            return
        raise AuthorityError("Registry source tag upload did not complete.")
    if response.headers.get("docker-content-digest") not in (None, digest):
        raise AuthorityError(
            "Registry response has a source tag digest that is different from the requested digest."
        )
    if not _head_manifest_reference(registry, transport, token, tag, digest, size):
        raise AuthorityError("Registry source tag is absent after upload.")
    verify_remote_manifest(
        registry, transport, token, manifest, digest, size, reference=tag
    )


def publish_image(registry, transport, token, manifest, blob_paths):
    """Publish every blob temp file and the platform manifest; prove every remote byte.

    ``blob_paths`` maps each manifest descriptor digest to the extracted temp
    file holding exactly those bytes (see ``extract_blob``). Blobs are
    streamed from disk for upload and compared chunk by chunk on readback;
    the complete image is never materialized in memory.

    Returns a publication proof binding each descriptor digest and the
    manifest digest to verified remote bytes. Anything missing, mismatched,
    or changed raises instead of reporting PASS.
    """
    if not isinstance(token, str) or not token:
        raise AuthorityError("Registry token is missing.")
    if (
        not isinstance(manifest, bytes)
        or len(manifest) < 1
        or len(manifest) > MAX_MANIFEST_BYTES
    ):
        raise AuthorityError("OCI manifest exceeds its size limit.")
    manifest_value = _json_bytes(manifest, "OCI manifest")
    expected = blob_descriptors(manifest_value)
    if not isinstance(blob_paths, dict) or set(blob_paths) != set(expected):
        raise AuthorityError(
            "Publication blob set is not the same as the OCI manifest descriptors."
        )
    for digest, size in expected.items():
        _require_blob_file(digest, blob_paths[digest], size)
    receipts = {}
    for digest in sorted(expected):
        receipts[digest] = publish_blob(
            registry, transport, token, digest, blob_paths[digest], expected[digest]
        )
    manifest_receipt = publish_manifest_receipt(registry, transport, token, manifest)
    for digest in sorted(expected):
        verify_remote_blob(
            registry, transport, token, digest, blob_paths[digest], expected[digest]
        )
    source_sha = getattr(registry, "SOURCE_SHA", None)
    if source_sha is not None:
        publish_source_tag(registry, transport, token, manifest, source_sha)
    return {"blobs": receipts, "manifest": manifest_receipt}


def check_publication_proof(proof, manifest):
    """Prove a publication proof covers exactly this manifest and its blobs."""
    if (
        not isinstance(manifest, bytes)
        or len(manifest) < 1
        or len(manifest) > MAX_MANIFEST_BYTES
    ):
        raise AuthorityError("OCI manifest exceeds its size limit.")
    manifest_value = _json_bytes(manifest, "OCI manifest")
    expected = blob_descriptors(manifest_value)
    if not isinstance(proof, dict) or set(proof) != {"blobs", "manifest"}:
        raise AuthorityError("Registry blob publication is not proven.")
    manifest_digest = "sha256:" + _sha(manifest)
    if proof["manifest"] != {
        "digest": manifest_digest,
        "size": len(manifest),
        "remote_sha256": manifest_digest.removeprefix("sha256:"),
    }:
        raise AuthorityError("Registry manifest publication is not proven.")
    blobs = proof["blobs"]
    if not isinstance(blobs, dict) or set(blobs) != set(expected):
        raise AuthorityError("Registry blob publication is not proven.")
    for digest, size in expected.items():
        if blobs[digest] != {
            "digest": digest,
            "size": size,
            "remote_sha256": digest.removeprefix("sha256:"),
        }:
            raise AuthorityError("Registry blob publication is not proven.")
    return proof


def build_evidence(identity, ci, record_archive, oci, manifest, record_sha256, record_source, publication):
    manifest_value = _json_bytes(manifest, "OCI manifest")
    if not isinstance(manifest_value, dict):
        raise AuthorityError("OCI platform manifest has an incorrect format.")
    source_commit = check_record_source(record_source, identity["source"])
    check_publication_proof(publication, manifest)
    config = manifest_value.get("config", {})
    layers = manifest_value.get("layers", [])
    evidence = {
        "schema": EVIDENCE_SCHEMA,
        "status": "PASS",
        "package": PACKAGE,
        "digest_reference": f"{PACKAGE}@{oci['platform_manifest_digest']}",
        "private_package_access_gate": "not_proven",
        "archive": {"sha256": record_archive["sha256"], "size_bytes": record_archive["size_bytes"]},
        "source": source_commit,
        "workflow": {
            "ref": identity["workflow_ref"],
            "authority_sha": identity["source"],
            "run_id": identity["run_id"],
            "run_attempt": identity["run_attempt"],
            "artifact_id": identity["artifact_id"],
        },
        "ci": ci,
        "oci": {
            "platform": "linux/amd64",
            "archive_index_digest": "sha256:" + oci["archive_index_digest"],
            "manifest_digest": oci["platform_manifest_digest"],
            "manifest_size_bytes": len(manifest),
            "config_digest": config.get("digest"),
            "layer_digests": [layer.get("digest") for layer in layers],
        },
        "record_sha256": record_sha256,
    }
    encoded = (json.dumps(evidence, sort_keys=True, separators=(",", ":")) + "\n").encode()
    if len(encoded) > MAX_RESULT_BYTES:
        raise AuthorityError("Publication evidence exceeds its size limit.")
    return encoded


def validate_evidence(value):
    expected_keys = {
        "schema", "status", "package", "digest_reference",
        "private_package_access_gate", "archive", "source", "workflow",
        "ci", "oci", "record_sha256",
    }
    if not isinstance(value, dict) or set(value) != expected_keys:
        raise AuthorityError("Publication evidence has unknown or missing fields.")
    if value["schema"] != EVIDENCE_SCHEMA or value["status"] != "PASS":
        raise AuthorityError("Publication evidence schema or status is incorrect.")
    if value["package"] != PACKAGE:
        raise AuthorityError("Publication evidence package is incorrect.")
    oci = value.get("oci")
    if not isinstance(oci, dict) or oci.get("platform") != "linux/amd64":
        raise AuthorityError("Publication evidence OCI platform is incorrect.")
    for name in ("manifest_digest", "config_digest"):
        digest = oci.get(name)
        if not isinstance(digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
            raise AuthorityError(f"Publication evidence {name} is incorrect.")
    if type(oci.get("manifest_size_bytes")) is not int or oci["manifest_size_bytes"] < 1:
        raise AuthorityError("Publication evidence manifest size is incorrect.")
    if (
        not isinstance(oci.get("layer_digests"), list)
        or not oci["layer_digests"]
        or any(not isinstance(item, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", item) for item in oci["layer_digests"])
    ):
        raise AuthorityError("Publication evidence layer digests are incorrect.")
    index_digest = oci.get("archive_index_digest")
    if not isinstance(index_digest, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", index_digest):
        raise AuthorityError("Publication evidence archive index digest is incorrect.")
    if value["digest_reference"] != f"{PACKAGE}@{oci['manifest_digest']}":
        raise AuthorityError("Publication evidence digest reference is incorrect.")
    if not isinstance(value.get("source"), str) or not re.fullmatch(r"[0-9a-f]{40}", value["source"]):
        raise AuthorityError("Publication evidence source is incorrect.")
    archive = value.get("archive")
    if (
        not isinstance(archive, dict)
        or not isinstance(archive.get("sha256"), str)
        or not re.fullmatch(r"[0-9a-f]{64}", archive["sha256"])
        or type(archive.get("size_bytes")) is not int
        or archive["size_bytes"] < 1
    ):
        raise AuthorityError("Publication evidence archive is incorrect.")
    workflow = value.get("workflow")
    if not isinstance(workflow, dict) or set(workflow) != {"ref", "authority_sha", "run_id", "run_attempt", "artifact_id"}:
        raise AuthorityError("Publication evidence workflow has unknown or missing fields.")
    if (
        not isinstance(workflow.get("ref"), str)
        or not workflow["ref"].endswith("@refs/heads/main")
        or not isinstance(workflow.get("authority_sha"), str)
        or not re.fullmatch(r"[0-9a-f]{40}", workflow["authority_sha"])
    ):
        raise AuthorityError("Publication evidence workflow identity is incorrect.")
    for name in ("run_id", "run_attempt", "artifact_id"):
        if type(workflow.get(name)) is not int or workflow[name] < 1:
            raise AuthorityError(f"Publication evidence workflow {name} is incorrect.")
    ci = value.get("ci")
    if (
        not isinstance(ci, dict)
        or set(ci) != {"run_id", "run_attempt"}
        or type(ci.get("run_id")) is not int
        or type(ci.get("run_attempt")) is not int
    ):
        raise AuthorityError("Publication evidence CI identity is incorrect.")
    if not isinstance(value.get("record_sha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", value["record_sha256"]):
        raise AuthorityError("Publication evidence record digest is incorrect.")
    if value.get("private_package_access_gate") != "not_proven":
        raise AuthorityError("Publication evidence access gate is incorrect.")
    return value
