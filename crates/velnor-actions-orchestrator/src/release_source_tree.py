"""Authenticated immutable Git source, without checkout or repository execution."""
import base64
import hashlib
import re
from types import MappingProxyType


_SOURCE_SEAL = object()
_SOURCE_SHA = re.compile(r"[0-9a-f]{40}")
_SOURCE_MAX_BLOB = 16 * 1024 * 1024
_SOURCE_MAX_TOTAL = 128 * 1024 * 1024


class _SourceTree:
    __slots__ = ("repository", "source_sha", "tree_sha", "entries", "_seal", "_blobs")

    def __init__(self, seal, repository, source_sha, tree_sha, entries):
        require(seal is _SOURCE_SEAL, "source_authority")
        for key, value in (("repository", repository), ("source_sha", source_sha),
                           ("tree_sha", tree_sha), ("entries", MappingProxyType(entries)),
                           ("_seal", seal), ("_blobs", {})):
            object.__setattr__(self, key, value)

    def __setattr__(self, name, value):
        raise AttributeError("immutable_source")


def _source_require_sha(value):
    require(type(value) is str and _SOURCE_SHA.fullmatch(value), "source_sha")
    return value


def _source_path(path):
    require(type(path) is str and path and not path.startswith("/") and
            "\\" not in path and len(path) <= 4096 and
            all(ord(character) >= 32 and not 127 <= ord(character) <= 159 for character in path),
            "source_path")
    require(all(part not in ("", ".", "..") and part.lower() != ".git"
                for part in path.split("/")), "source_path")
    try:
        encoded = path.encode("utf-8")
    except UnicodeError as error:
        raise ReconcileError("source_path_utf8") from error
    require(len(encoded) <= 4096, "source_path_size")
    return encoded


def _source_entries(response, tree_sha):
    require(isinstance(response, dict) and response.get("sha") == tree_sha and
            response.get("truncated") is False and type(response.get("tree")) is list and
            len(response["tree"]) <= 20000, "source_tree_response")
    entries = {}
    for item in response["tree"]:
        require(type(item) is dict, "source_tree_entry")
        path = item.get("path")
        _source_path(path)
        require(path not in entries, "source_tree_duplicate")
        sha = _source_require_sha(item.get("sha"))
        kind, mode = item.get("type"), item.get("mode")
        require(type(kind) is str and type(mode) is str and
                (kind, mode) in {("tree", "040000"), ("blob", "100644"),
                                ("blob", "100755")}, "source_tree_mode")
        size = item.get("size")
        require(kind == "tree" or type(size) is int and 0 <= size <= _SOURCE_MAX_BLOB,
                "source_tree_blob_size")
        entries[path] = MappingProxyType({"sha": sha, "mode": mode, "type": kind,
                                          **({"size": size} if kind == "blob" else {})})
    return entries


def _source_verify_trees(entries, tree_sha):
    children = {"": []}
    for path, entry in entries.items():
        if entry["type"] == "tree":
            children[path] = []
    for path, entry in entries.items():
        parent, _, name = path.rpartition("/")
        require(parent in children, "source_tree_parent")
        children[parent].append((name.encode("utf-8"), entry))
    for directory, items in children.items():
        items.sort(key=lambda item: item[0] + (b"/" if item[1]["type"] == "tree" else b""))
        raw = b"".join((b"40000" if entry["type"] == "tree" else entry["mode"].encode()) +
                       b" " + name + b"\0" + bytes.fromhex(entry["sha"])
                       for name, entry in items)
        actual = hashlib.sha1(b"tree " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        expected = entries[directory]["sha"] if directory else tree_sha
        require(actual == expected, "source_tree_digest")


def source_tree(approved):
    """Freshly read exact approved commit and verify its complete Git tree."""
    require(type(approved) is dict, "source_policy")
    repository, source_sha = approved.get("repository"), approved.get("source_sha")
    require(type(repository) is str and
            re.fullmatch(r"[A-Za-z0-9._-]+/[A-Za-z0-9._-]+", repository), "source_repository")
    _source_require_sha(source_sha)
    commit = read_source_commit(repository, source_sha)
    require(type(commit) is dict and commit.get("sha") == source_sha and
            type(commit.get("tree")) is dict, "source_commit")
    tree_sha = _source_require_sha(commit["tree"].get("sha"))
    entries = _source_entries(read_source_tree(repository, tree_sha), tree_sha)
    _source_verify_trees(entries, tree_sha)
    require(sum(entry.get("size", 0) for entry in entries.values()) <= _SOURCE_MAX_TOTAL,
            "source_tree_total_size")
    return _SourceTree(_SOURCE_SEAL, repository, source_sha, tree_sha, entries)


def _source_authority(source, limit):
    require(type(source) is _SourceTree and source._seal is _SOURCE_SEAL, "source_authority")
    require(type(limit) is int and 0 <= limit <= _SOURCE_MAX_BLOB, "source_blob_limit")


def source_blob(source, entry, limit):
    """Return exact bounded Git blob bytes belonging to this fresh source."""
    _source_authority(source, limit)
    require(any(value is entry for value in source.entries.values()) and
            entry["type"] == "blob" and entry["size"] <= limit, "source_blob_entry")
    sha = entry["sha"]
    if sha not in source._blobs:
        blob = read_source_blob(source.repository, sha)
        require(type(blob) is dict and blob.get("sha") == sha and
                blob.get("encoding") == "base64" and type(blob.get("content")) is str and
                type(blob.get("size")) is int and blob["size"] == entry["size"],
                "source_blob_response")
        encoded = blob["content"]
        require(len(encoded) <= ((entry["size"] + 2) // 3) * 4 * 2 + 2,
                "source_blob_encoding_size")
        try:
            raw = base64.b64decode(encoded.replace("\n", ""), validate=True)
        except ValueError as error:
            raise ReconcileError("source_blob_encoding") from error
        require(len(raw) == entry["size"] and len(raw) <= limit, "source_blob_size")
        actual = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
        require(actual == sha, "source_blob_digest")
        source._blobs[sha] = raw
    raw = source._blobs[sha]
    require(type(raw) is bytes and len(raw) == entry["size"] and len(raw) <= limit,
            "source_blob_size")
    actual = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
    require(actual == sha, "source_blob_digest")
    return raw


def source_file(source, path, limit=1024 * 1024):
    """Read authenticated manifest/changelog/source bytes; never follow links."""
    _source_authority(source, limit)
    _source_path(path)
    require(path in source.entries, "source_file_missing")
    return source_blob(source, source.entries[path], limit)
