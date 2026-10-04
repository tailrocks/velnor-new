"""Anonymous trusted preparation entry; its immutable result grants no authority."""
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import tempfile
import unicodedata
import zipfile


class PreparedSourceIntent:
    __slots__ = ("data", "sha256")

    def __init__(self, data):
        _intent_require(type(data) is bytes, "prepared_result_bytes")
        object.__setattr__(self, "data", data)
        object.__setattr__(self, "sha256", hashlib.sha256(data).hexdigest())

    def __setattr__(self, name, value):
        raise AttributeError("immutable prepared bytes")


def _prepared_original(descriptor, filename, expected, remaining):
    handle = os.open(filename, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                     dir_fd=descriptor)
    try:
        before = os.fstat(handle)
        _intent_require(stat.S_ISREG(before.st_mode) and
                        0 <= before.st_size <= min(PREPARED_MAX_ARCHIVE_BYTES, remaining),
                        "prepared_original_size_or_kind")
        chunks, total = [], 0
        while True:
            chunk = os.read(handle, min(1024 * 1024, before.st_size + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            _intent_require(total <= before.st_size, "prepared_original_changed")
        after = os.fstat(handle)
        _intent_require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                         before.st_ctime_ns) == (after.st_dev, after.st_ino, after.st_size,
                                                after.st_mtime_ns, after.st_ctime_ns) and
                        total == before.st_size, "prepared_original_changed")
        data = b"".join(chunks)
        _intent_require(hashlib.sha256(data).hexdigest() == expected,
                        "prepared_original_digest")
        return data
    finally:
        os.close(handle)


def _prepared_originals(packages, directory):
    _intent_require(0 < len(packages) <= PREPARED_MAX_PACKAGES, "prepared_package_count")
    descriptor = os.open(directory / "crates", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        members, aliases, total = {}, set(), 0
        for name, package in sorted(packages.items()):
            metadata = package["publish_metadata"]
            filename = f"{metadata['name']}-{metadata['vers']}.crate"
            _intent_require(metadata["name"] == name and
                            Path(filename).name == filename and "/" not in filename and
                            "\\" not in filename, "prepared_original_identity")
            member = "crates/" + filename
            alias = unicodedata.normalize("NFC", member).casefold()
            _intent_require(alias not in aliases, "prepared_original_alias")
            aliases.add(alias)
            data = _prepared_original(descriptor, filename, package["archive_sha256"],
                                      PREPARED_MAX_TOTAL_ARCHIVE_BYTES - total)
            total += len(data)
            members[member] = data
        _intent_require(set(os.listdir(descriptor)) ==
                        {name.removeprefix("crates/") for name in members},
                        "prepared_original_coverage")
        return members
    finally:
        os.close(descriptor)


def _prepared_zip(evidence, originals):
    _intent_require(type(originals) is dict and len(originals) <= PREPARED_MAX_PACKAGES and
                    all(type(data) is bytes and len(data) <= PREPARED_MAX_ARCHIVE_BYTES
                        for data in originals.values()) and
                    sum(map(len, originals.values())) <= PREPARED_MAX_TOTAL_ARCHIVE_BYTES,
                    "prepared_zip_original_bounds")
    raw = json.dumps(evidence, sort_keys=True, separators=(",", ":"),
                     ensure_ascii=False, allow_nan=False).encode("utf-8")
    _intent_require(len(raw) <= PREPARED_MAX_EVIDENCE_BYTES, "prepared_evidence_size")
    members = {"evidence.json": raw, **originals}
    _intent_require(len(members) <= PREPARED_MAX_ZIP_MEMBERS, "prepared_zip_members")
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_STORED) as archive:
        for name, data in sorted(members.items()):
            entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            entry.create_system = 3
            entry.external_attr = (stat.S_IFREG | 0o444) << 16
            archive.writestr(entry, data)
    data = stream.getvalue()
    _intent_require(len(data) <= PREPARED_MAX_ZIP_BYTES, "prepared_zip_size")
    return PreparedSourceIntent(data)


def prepare_source_intent():
    """One fixed source-only operation; missing compiled capabilities deny execution."""
    sdk_type = _intent_dependency("ColdSourceIntentSdk")
    _intent_dependency("ColdSourceIntentInstalledTool")
    sdk_loader = _intent_dependency("load_source_intent_sdk")
    sdk = sdk_loader()
    _intent_require(type(sdk) is sdk_type, "source_intent_sdk_capability")
    context_type = _intent_dependency("_CompiledPreparedContext")
    sink_type = _intent_dependency("_PreparedOutputSink")
    context = _intent_dependency("compiled_prepared_context")()
    _intent_require(type(context) is context_type and type(context.output_sink) is sink_type,
                    "prepared_context_capability")
    approved = context.approved
    sdk.require_policy(approved["tools"]["rust"], context.actual_host)
    source = _intent_dependency("load_authenticated_source_snapshot")()
    source_descriptor = _intent_dependency("authenticated_source_descriptor")(source)
    with tempfile.TemporaryDirectory(prefix="velnor-prepared-originals-",
                                     dir=Path("/tmp").resolve(strict=True)) as temporary:
        directory = Path(temporary)
        packages = source_intent_inventory(approved, context.manifest, sdk, context.actual_host,
                                           directory, context.release_config, source_descriptor)
        evidence = {"schema": 1, "kind": "source-intent-prepared", "policy": approved,
                    "source_snapshot": source_descriptor, "packages": packages,
                    "publication_order": selected_publication_order(packages)}
        _intent_dependency("validate_prepared_evidence")(evidence, approved, source)
        result = _prepared_zip(evidence, _prepared_originals(packages, directory))
    custodied_digest = context.write_prepared_payload(result.data)
    _intent_require(custodied_digest == result.sha256, "prepared_output_payload_digest")
    context.output_sink.publish_prepared_sha256(result.sha256)
    return result
