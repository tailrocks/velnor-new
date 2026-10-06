"""Original-source input bridge; unregistered until its compiled owner qualifies."""
import hashlib
import os
from pathlib import Path
import re
import stat
from types import MappingProxyType


_AUTHENTICATED_SNAPSHOT_SEAL = object()
_SOURCE_INPUT_FIELDS = frozenset({
    "repository", "source_sha", "tree_sha", "workflow", "workflow_sha", "ref", "run_id", "attempt",
    "producer_job", "producer_helper_sha256", "artifact_id", "raw_zip_sha256",
    "inner_sha256", "destination"})
_SOURCE_DESCRIPTOR_FIELDS = _SOURCE_INPUT_FIELDS - {"destination"}


class _AuthenticatedSourceSnapshot:
    __slots__ = ("_snapshot", "transport", "_seal")

    def __init__(self, seal, snapshot, binding):
        require(seal is _AUTHENTICATED_SNAPSHOT_SEAL, "source_artifact_authority")
        object.__setattr__(self, "_snapshot", snapshot)
        object.__setattr__(self, "transport", MappingProxyType(dict(binding)))
        object.__setattr__(self, "_seal", seal)

    def __setattr__(self, name, value):
        raise AttributeError("immutable_authenticated_source_snapshot")

    @property
    def repository(self):
        return self._snapshot.repository

    @property
    def source_sha(self):
        return self._snapshot.source_sha

    @property
    def tree_sha(self):
        return self._snapshot.tree_sha

    @property
    def entries(self):
        return self._snapshot.entries


def _compiled_source_artifact_input():
    # No environment dictionary, descriptor, callback, or caller assertion can
    # replace actual compiled producer-graph/launcher qualification.
    raise ReconcileError("source_artifact_compiled_input_unqualified")


def _source_artifact_content_binding(binding):
    """Check transport/content shape only. This function grants no authority."""
    require(type(binding) is dict and set(binding) == _SOURCE_INPUT_FIELDS,
            "source_artifact_binding_fields")
    _source_artifact_identity_fields(binding)
    require(type(binding["destination"]) is str and
            Path(binding["destination"]).is_absolute(), "source_artifact_destination")
    _source_path(binding["destination"][1:])


def _source_artifact_identity_fields(binding):
    require(type(binding["repository"]) is str and
            re.fullmatch(r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+", binding["repository"]),
            "source_artifact_repository")
    for key in ("source_sha", "tree_sha", "workflow_sha"):
        _source_require_sha(binding[key])
    for key in ("producer_helper_sha256", "raw_zip_sha256", "inner_sha256"):
        require(type(binding[key]) is str and re.fullmatch(r"[0-9a-f]{64}", binding[key]),
                "source_artifact_digest")
    for key in ("run_id", "attempt", "artifact_id"):
        require(type(binding[key]) is str and re.fullmatch(r"[1-9][0-9]*", binding[key]),
                "source_artifact_natural")
    require(binding["producer_job"] == "release-source-snapshot" and
            binding["workflow"] == ".github/workflows/release.yml" and
            type(binding["ref"]) is str and binding["ref"].startswith("refs/"),
            "source_artifact_producer")
    _source_path(binding["workflow"])


def validate_source_snapshot_descriptor(value):
    """Validate portable claims only; JSON descriptors never mint source authority."""
    require(type(value) is dict and set(value) == _SOURCE_DESCRIPTOR_FIELDS,
            "source_snapshot_descriptor_fields")
    _source_artifact_identity_fields(value)


def authenticated_source_descriptor(source):
    """Return portable JSON claims from the genuine original source capability."""
    authenticated_source_snapshot(source, _SOURCE_MAX_BLOB)
    require((source.repository, source.source_sha, source.tree_sha) ==
            (source.transport["repository"], source.transport["source_sha"],
             source.transport["tree_sha"]), "source_snapshot_descriptor_identity")
    value = {key: source.transport[key] for key in sorted(_SOURCE_DESCRIPTOR_FIELDS)}
    validate_source_snapshot_descriptor(value)
    return value


def _source_artifact_read_content(binding):
    """Read one exact inner ZIP without links; outer service ZIP remains a claim."""
    _source_artifact_content_binding(binding)
    path = Path(binding["destination"])
    root = os.open(path.anchor, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for part in path.parts[1:]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=root)
            os.close(root)
            root = child
        require(set(os.listdir(root)) == {"snapshot.zip"}, "source_artifact_layout")
        descriptor = os.open("snapshot.zip", os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                             dir_fd=root)
        try:
            before = os.fstat(descriptor)
            require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and
                    before.st_uid == os.getuid() and 0 < before.st_size <= _SNAPSHOT_MAX_ZIP,
                    "source_artifact_file")
            chunks, total = [], 0
            while True:
                chunk = os.read(descriptor, min(1024 * 1024, _SNAPSHOT_MAX_ZIP + 1 - total))
                if not chunk:
                    break
                chunks.append(chunk)
                total += len(chunk)
                require(total <= _SNAPSHOT_MAX_ZIP, "source_artifact_size")
            after = os.fstat(descriptor)
            require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns,
                     before.st_ctime_ns) == (after.st_dev, after.st_ino, after.st_size,
                                            after.st_mtime_ns, after.st_ctime_ns) and
                    total == before.st_size, "source_artifact_changed")
            return b"".join(chunks)
        finally:
            os.close(descriptor)
    finally:
        os.close(root)


def _source_artifact_decode_content(raw, binding):
    """Validate same consumed bytes and expected Git root; return pure content."""
    _source_artifact_content_binding(binding)
    require(type(raw) is bytes and hashlib.sha256(raw).hexdigest() == binding["inner_sha256"],
            "source_artifact_inner_digest")
    snapshot = decode_source_snapshot(raw, {"repository": binding["repository"],
                                           "source_sha": binding["source_sha"]})
    require(snapshot.tree_sha == binding["tree_sha"], "source_artifact_expected_tree")
    return snapshot


def load_authenticated_source_snapshot():
    """Only actual compiled owner qualification may activate this zero-arg loader."""
    binding = _compiled_source_artifact_input()
    raw = _source_artifact_read_content(binding)
    snapshot = _source_artifact_decode_content(raw, binding)
    return _AuthenticatedSourceSnapshot(_AUTHENTICATED_SNAPSHOT_SEAL, snapshot, binding)


def authenticated_source_snapshot(source, limit):
    require(type(source) is _AuthenticatedSourceSnapshot and
            source._seal is _AUTHENTICATED_SNAPSHOT_SEAL, "source_artifact_authority")
    require(type(limit) is int and 0 <= limit <= _SOURCE_MAX_BLOB, "source_blob_limit")


def authenticated_source_blob(source, entry, limit):
    authenticated_source_snapshot(source, limit)
    require(any(value is entry for value in source.entries.values()) and
            entry["type"] == "blob" and entry["size"] <= limit, "source_blob_entry")
    return _materialize_blob(source._snapshot, entry)


def authenticated_source_file(source, path, limit=1024 * 1024):
    """Read local authenticated original source without weakening API SourceTree."""
    authenticated_source_snapshot(source, limit)
    _source_path(path)
    require(path in source.entries, "source_file_missing")
    return authenticated_source_blob(source, source.entries[path], limit)


def materialize_authenticated_source_snapshot(source, destination):
    authenticated_source_snapshot(source, _SOURCE_MAX_BLOB)
    return materialize_source_snapshot(source._snapshot, destination)


def _materialize_authenticated_source_owned(source, destination):
    """Private same-FD materialization; source authority stays in the genuine cap."""
    authenticated_source_snapshot(source, _SOURCE_MAX_BLOB)
    return _materialize_source_snapshot_owned(source._snapshot, destination)
