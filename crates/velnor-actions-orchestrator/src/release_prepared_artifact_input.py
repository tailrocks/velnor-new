"""Unregistered prepared input; only its genuine compiled owner may activate it."""
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
from types import MappingProxyType
import zipfile
import zlib


_ACTUAL_PREPARED_SEAL = object()
_PREPARED_INPUT_FIELDS = frozenset({
    "approved", "repository", "source_sha", "workflow", "workflow_sha", "ref",
    "run_id", "attempt", "producer_job", "producer_helper_sha256", "artifact_id",
    "raw_zip_sha256", "inner_sha256", "destination"})


class _ActualPreparedHandle:
    __slots__ = ("raw_zip", "archives", "source", "transport", "_evidence_json", "_seal")

    def __init__(self, seal, raw, archives, evidence, source, binding):
        require(seal is _ACTUAL_PREPARED_SEAL, "prepared_artifact_authority")
        object.__setattr__(self, "raw_zip", raw)
        object.__setattr__(self, "archives", MappingProxyType(dict(archives)))
        object.__setattr__(self, "source", source)
        object.__setattr__(self, "transport", _prepared_readonly(binding))
        object.__setattr__(self, "_evidence_json", json.dumps(evidence, allow_nan=False))
        object.__setattr__(self, "_seal", seal)

    def __setattr__(self, name, value):
        raise AttributeError("immutable_actual_prepared_handle")

    @property
    def evidence(self):
        return decode_json(self._evidence_json)

    @property
    def approved(self):
        return self.evidence["policy"]


def _prepared_readonly(value):
    if type(value) is dict:
        return MappingProxyType({key: _prepared_readonly(item) for key, item in value.items()})
    if type(value) is list:
        return tuple(_prepared_readonly(item) for item in value)
    return value


def _compiled_prepared_artifact_input():
    # Neither caller JSON nor a transport-shaped dictionary qualifies this owner.
    raise ReconcileError("prepared_artifact_compiled_input_unqualified")


def _prepared_content_binding(binding):
    """Validate transport claims only; this function grants no capability."""
    require(type(binding) is dict and set(binding) == _PREPARED_INPUT_FIELDS,
            "prepared_artifact_binding_fields")
    require(type(binding["approved"]) is dict and
            type(binding["repository"]) is str and
            re.fullmatch(r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+", binding["repository"]),
            "prepared_artifact_policy")
    for key in ("source_sha", "workflow_sha"):
        require(type(binding[key]) is str and re.fullmatch(r"[0-9a-f]{40}", binding[key]),
                "prepared_artifact_sha")
    for key in ("producer_helper_sha256", "raw_zip_sha256", "inner_sha256"):
        require(type(binding[key]) is str and re.fullmatch(r"[0-9a-f]{64}", binding[key]),
                "prepared_artifact_digest")
    for key in ("run_id", "attempt", "artifact_id"):
        require(type(binding[key]) is str and re.fullmatch(r"[1-9][0-9]*", binding[key]),
                "prepared_artifact_natural")
    require(binding["workflow"] == ".github/workflows/release.yml" and
            type(binding["ref"]) is str and binding["ref"].startswith("refs/") and
            binding["producer_job"] == "release-package",
            "prepared_artifact_producer_claim")
    require(binding["approved"].get("repository") == binding["repository"] and
            binding["approved"].get("source_sha") == binding["source_sha"],
            "prepared_artifact_policy_identity")
    path = binding["destination"]
    require(type(path) is str and path.startswith("/") and "\\" not in path and
            all(part not in ("", ".", "..") for part in path.split("/")[1:]),
            "prepared_artifact_destination")


def _prepared_zip_contents(raw):
    require(type(raw) is bytes and 0 < len(raw) <= PREPARED_MAX_ZIP_BYTES,
            "prepared_artifact_zip_size")
    contents, total = {}, 0
    try:
        with zipfile.ZipFile(io.BytesIO(raw)) as archive:
            members = archive.infolist()
            require(0 < len(members) <= PREPARED_MAX_ZIP_MEMBERS and not archive.comment,
                    "prepared_artifact_zip_members")
            for member in members:
                name = member.filename
                require(member.orig_filename == name and name not in contents and
                        not member.is_dir() and
                        not member.flag_bits & 1 and not member.comment and not member.extra and
                        member.compress_type == zipfile.ZIP_STORED and
                        (member.external_attr >> 16) & 0o170000 in (0, stat.S_IFREG),
                        "prepared_artifact_zip_member")
                bound = (PREPARED_MAX_EVIDENCE_BYTES if name == "evidence.json"
                         else PREPARED_MAX_ARCHIVE_BYTES)
                require(0 <= member.file_size <= bound and
                        member.compress_size == member.file_size, "prepared_artifact_member_size")
                with archive.open(member) as stream:
                    data = stream.read(bound + 1)
                require(len(data) == member.file_size and len(data) <= bound,
                        "prepared_artifact_member_truncated")
                if name != "evidence.json":
                    total += len(data)
                    require(total <= PREPARED_MAX_TOTAL_ARCHIVE_BYTES,
                            "prepared_artifact_total_size")
                contents[name] = data
    except (zipfile.BadZipFile, OSError, RuntimeError, EOFError, UnicodeError, zlib.error) as error:
        raise ReconcileError("prepared_artifact_zip_invalid") from error
    return contents


def decode_prepared_package(raw, binding):
    """Decode one bound original buffer into content claims; never mint a handle."""
    _prepared_content_binding(binding)
    require(type(raw) is bytes and 0 < len(raw) <= PREPARED_MAX_ZIP_BYTES and
            hashlib.sha256(raw).hexdigest() == binding["inner_sha256"],
            "prepared_artifact_inner_digest")
    contents = _prepared_zip_contents(raw)
    require("evidence.json" in contents, "prepared_artifact_evidence_missing")
    evidence = decode_json(contents["evidence.json"])
    require(type(evidence) is dict and "source_snapshot" in evidence,
            "prepared_artifact_evidence_claims")
    validate_prepared_shape(evidence, binding["approved"], evidence["source_snapshot"])
    archives = {}
    expected = {"evidence.json"}
    for name, version in binding["approved"]["packages"].items():
        filename = f"crates/{name}-{version}.crate"
        expected.add(filename)
        require(filename in contents, "prepared_artifact_archive_missing")
        data = contents[filename]
        require(hashlib.sha256(data).hexdigest() == evidence["packages"][name]["archive_sha256"],
                "prepared_artifact_archive_digest")
        archives[name] = data
    require(set(contents) == expected, "prepared_artifact_exact_members")
    return evidence, archives


def _prepared_input_path(binding):
    workspace = os.environ.get("GITHUB_WORKSPACE", "")
    require(type(workspace) is str and workspace.startswith("/") and
            all(part not in ("", ".", "..") for part in workspace.split("/")[1:]),
            "prepared_artifact_workspace")
    root = Path(workspace)
    require(root.resolve(strict=True) == root and root.is_dir(),
            "prepared_artifact_workspace_noncanonical")
    expected = str(root / "release-prepared-input")
    require(binding["destination"] == expected, "prepared_artifact_pinned_destination")
    return Path(expected)


def _prepared_read_file(root):
    descriptor = os.open("prepared.zip", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                         dir_fd=root)
    try:
        before = os.fstat(descriptor)
        require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and
                before.st_uid == os.getuid() and 0 < before.st_size <= PREPARED_MAX_ZIP_BYTES,
                "prepared_artifact_file")
        chunks, total = [], 0
        while True:
            chunk = os.read(descriptor, min(1024 * 1024, PREPARED_MAX_ZIP_BYTES + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            require(total <= PREPARED_MAX_ZIP_BYTES, "prepared_artifact_file_size")
        after = os.fstat(descriptor)
        require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                 before.st_ctime_ns) == (after.st_dev, after.st_ino, after.st_size,
                                        after.st_mtime_ns, after.st_ctime_ns) and
                total == before.st_size, "prepared_artifact_changed")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def _prepared_read_content(binding):
    _prepared_content_binding(binding)
    path = _prepared_input_path(binding)
    root = os.open(path.anchor, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for part in path.parts[1:]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=root)
            os.close(root)
            root = child
        require(set(os.listdir(root)) == {"prepared.zip"}, "prepared_artifact_layout")
        return _prepared_read_file(root)
    finally:
        os.close(root)


def load_authenticated_prepared_package():
    """Only the genuine compiled Prepared input owner may activate this loader."""
    binding = _compiled_prepared_artifact_input()
    raw = _prepared_read_content(binding)
    evidence, archives = decode_prepared_package(raw, binding)
    source = load_authenticated_source_snapshot()
    validate_prepared_evidence(evidence, binding["approved"], source)
    descriptor = authenticated_source_descriptor(source)
    require(all(same_json(descriptor[key], binding[key]) for key in (
        "repository", "source_sha", "workflow", "workflow_sha", "ref", "run_id", "attempt")),
        "prepared_artifact_source_context")
    return _ActualPreparedHandle(_ACTUAL_PREPARED_SEAL, raw, archives, evidence, source, binding)


def authenticated_prepared_package(handle):
    require(type(handle) is _ActualPreparedHandle and handle._seal is _ACTUAL_PREPARED_SEAL,
            "prepared_artifact_authority")
    return handle
