"""Pure source snapshot codec: validated content carries no producer authority."""
import hashlib
import io
import json
import re
import stat
from types import MappingProxyType
import zipfile
import zlib


_SNAPSHOT_SEAL = object()
_SNAPSHOT_MAX_ZIP = 256 * 1024 * 1024
_SNAPSHOT_MAX_MANIFEST = 8 * 1024 * 1024
_SNAPSHOT_MAX_EXPANDED = 136 * 1024 * 1024


class _SourceSnapshot:
    """Immutable validated content; callers must authenticate its producer separately."""
    __slots__ = ("repository", "source_sha", "tree_sha", "entries", "blobs", "_seal")

    def __init__(self, seal, repository, source_sha, tree_sha, entries, blobs):
        require(seal is _SNAPSHOT_SEAL, "source_snapshot_content")
        for key, value in (("repository", repository), ("source_sha", source_sha),
                           ("tree_sha", tree_sha), ("entries", MappingProxyType(entries)),
                           ("blobs", MappingProxyType(blobs)), ("_seal", seal)):
            object.__setattr__(self, key, value)

    def __setattr__(self, name, value):
        raise AttributeError("immutable_source_snapshot")


def _snapshot_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "source_snapshot_json_duplicate")
        result[key] = value
    return result


def _snapshot_manifest(raw, approved):
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_snapshot_object,
                           parse_constant=lambda _: require(False, "source_snapshot_json_number"))
    except ReconcileError:
        raise
    except (UnicodeError, ValueError, RecursionError) as error:
        raise ReconcileError("source_snapshot_json") from error
    require(type(approved) is dict and type(value) is dict and set(value) == {
        "schema", "repository", "source_sha", "tree_sha", "entries"} and
            type(value["schema"]) is int and value["schema"] == 1, "source_snapshot_schema")
    require(type(value["repository"]) is str and
            re.fullmatch(r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+", value["repository"]) and
            value["repository"] == approved.get("repository") and
            value["source_sha"] == approved.get("source_sha"), "source_snapshot_policy")
    _source_require_sha(value["source_sha"])
    _source_require_sha(value["tree_sha"])
    mapping = value["entries"]
    require(type(mapping) is dict and len(mapping) <= 20000, "source_snapshot_entries")
    items = []
    for path, entry in mapping.items():
        require(type(entry) is dict and set(entry) ==
                ({"sha", "type", "mode", "size"} if entry.get("type") == "blob"
                 else {"sha", "type", "mode"}), "source_snapshot_entry_fields")
        items.append({"path": path, **entry})
    entries = _source_entries({"sha": value["tree_sha"], "truncated": False, "tree": items},
                              value["tree_sha"])
    _source_verify_trees(entries, value["tree_sha"])
    require(sum(entry.get("size", 0) for entry in entries.values()) <= _SOURCE_MAX_TOTAL,
            "source_snapshot_total_size")
    return value, entries


def _snapshot_zip_contents(raw):
    require(type(raw) is bytes and 0 < len(raw) <= _SNAPSHOT_MAX_ZIP,
            "source_snapshot_zip_size")
    contents, total = {}, 0
    try:
        with zipfile.ZipFile(io.BytesIO(raw)) as archive:
            members = archive.infolist()
            require(1 <= len(members) <= 20001, "source_snapshot_member_count")
            for member in members:
                name = member.filename
                require(member.orig_filename == name and
                        (name == "evidence.json" or re.fullmatch(r"blobs/[0-9a-f]{40}", name)),
                        "source_snapshot_member_path")
                require(name not in contents and not member.is_dir() and
                        not member.flag_bits & 1 and
                        stat.S_IFMT(member.external_attr >> 16) in (0, stat.S_IFREG) and
                        member.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                        "source_snapshot_member_kind")
                bound = _SNAPSHOT_MAX_MANIFEST if name == "evidence.json" else _SOURCE_MAX_BLOB
                require(0 <= member.file_size <= bound, "source_snapshot_member_size")
                total += member.file_size
                require(total <= _SNAPSHOT_MAX_EXPANDED, "source_snapshot_expansion")
                with archive.open(member) as stream:
                    content = stream.read(bound + 1)
                require(len(content) == member.file_size, "source_snapshot_member_truncated")
                contents[name] = content
    except (zipfile.BadZipFile, RuntimeError, OSError, EOFError, UnicodeError, zlib.error) as error:
        raise ReconcileError("source_snapshot_zip") from error
    require("evidence.json" in contents, "source_snapshot_manifest_missing")
    return contents


def decode_source_snapshot(raw, approved):
    """Validate pure snapshot bytes; do not grant authenticated source authority."""
    contents = _snapshot_zip_contents(raw)
    value, entries = _snapshot_manifest(contents.pop("evidence.json"), approved)
    expected = {"blobs/" + entry["sha"] for entry in entries.values() if entry["type"] == "blob"}
    require(set(contents) == expected, "source_snapshot_blob_coverage")
    blobs = {}
    for entry in entries.values():
        if entry["type"] != "blob":
            continue
        content, sha = contents["blobs/" + entry["sha"]], entry["sha"]
        require(type(content) is bytes and len(content) == entry["size"],
                "source_snapshot_blob_size")
        actual = hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest()
        require(actual == sha, "source_snapshot_blob_digest")
        blobs[sha] = content
    return _SourceSnapshot(_SNAPSHOT_SEAL, value["repository"], value["source_sha"],
                           value["tree_sha"], entries, blobs)


def _snapshot_write_member(archive, name, raw):
    member = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    member.create_system = 3
    member.external_attr = (stat.S_IFREG | 0o400) << 16
    member.compress_type = zipfile.ZIP_STORED
    archive.writestr(member, raw)


def serialize_source_snapshot(source):
    """Freeze all freshly authenticated tree/blob bytes into one deterministic ZIP."""
    _source_authority(source, _SOURCE_MAX_BLOB)
    blobs = {}
    for entry in source.entries.values():
        if entry["type"] == "blob":
            blobs[entry["sha"]] = source_blob(source, entry, _SOURCE_MAX_BLOB)
    manifest = {"schema": 1, "repository": source.repository, "source_sha": source.source_sha,
                "tree_sha": source.tree_sha,
                "entries": {path: dict(entry) for path, entry in source.entries.items()}}
    raw = json.dumps(manifest, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")
    require(len(raw) <= _SNAPSHOT_MAX_MANIFEST, "source_snapshot_manifest_size")
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        _snapshot_write_member(archive, "evidence.json", raw)
        for sha, content in sorted(blobs.items()):
            _snapshot_write_member(archive, "blobs/" + sha, content)
    result = buffer.getvalue()
    require(len(result) <= _SNAPSHOT_MAX_ZIP, "source_snapshot_zip_size")
    return result


def compare_source_snapshot(source, snapshot):
    """Compare parsed content with independently refreshed authenticated API source."""
    _source_authority(source, _SOURCE_MAX_BLOB)
    require(type(snapshot) is _SourceSnapshot and snapshot._seal is _SNAPSHOT_SEAL,
            "source_snapshot_content")
    require((source.repository, source.source_sha, source.tree_sha) ==
            (snapshot.repository, snapshot.source_sha, snapshot.tree_sha) and
            source.entries == snapshot.entries, "source_snapshot_live_binding")
    expected = {entry["sha"] for entry in source.entries.values() if entry["type"] == "blob"}
    require(set(snapshot.blobs) == expected, "source_snapshot_blob_coverage")
    for entry in source.entries.values():
        if entry["type"] == "blob":
            raw = source_blob(source, entry, _SOURCE_MAX_BLOB)
            require(type(snapshot.blobs[entry["sha"]]) is bytes and
                    raw == snapshot.blobs[entry["sha"]], "source_snapshot_live_blob")
