"""Strict verifier for the reviewed cargo-semver-checks Git tar capsule."""

import hashlib
import io
import posixpath
import tarfile
import unicodedata


ARCHIVE_LIMIT = 32 * 1024 * 1024
EXPANDED_LIMIT = 64 * 1024 * 1024
ENTRY_LIMIT = 5000
EXPECTED_PAX = "583dddce84706786fc54c41a2c768c28a09c65fd"
EXPECTED_COMMIT = EXPECTED_PAX
EXPECTED_TREE = "b0f6ea8b85ac0ed288fc29996e441aaa61bbab48"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git_object(kind, data):
    header = kind.encode() + b" " + str(len(data)).encode() + b"\0"
    return hashlib.sha1(header + data).hexdigest()


def portable_path(name):
    require(isinstance(name, str) and name, "archive path must be a nonempty UTF-8 string")
    require("\x00" not in name and "\\" not in name and not name.startswith("/"),
            "archive path is not portable")
    try:
        encoded = name.encode("utf-8")
        require(encoded.decode("utf-8") == name, "archive path is not valid UTF-8")
    except UnicodeError as error:
        raise ValueError("archive path is not valid UTF-8") from error
    require(unicodedata.normalize("NFC", name) == name and
            unicodedata.normalize("NFKC", name) == name, "archive path has Unicode alias")
    parts = name.split("/")
    require(all(part not in ("", ".", "..") for part in parts),
            "archive path has unsafe components")
    require(".git" not in parts, "archive path contains Git metadata")
    require(posixpath.normpath(name) == name, "archive path is not normalized")
    return name


def _inventory(inventory, revision):
    require(isinstance(inventory, dict) and set(inventory) ==
            {"schema_version", "source_commit", "source_tree", "files"},
            "semver source inventory fields must be closed")
    require(type(inventory["schema_version"]) is int and inventory["schema_version"] == 1 and
            inventory["source_commit"] == revision.source_commit and
            inventory["source_tree"] == revision.source_tree and isinstance(inventory["files"], dict),
            "semver source inventory identity differs")
    require(0 < len(inventory["files"]) <= ENTRY_LIMIT, "semver source inventory size invalid")
    result = {}
    aliases = set()
    for name, metadata in inventory["files"].items():
        normalized = portable_path(name)
        key = unicodedata.normalize("NFKC", normalized).casefold()
        require(key not in aliases, "semver inventory has a path alias")
        aliases.add(key)
        require(isinstance(metadata, dict) and set(metadata) ==
                {"sha256", "git_blob", "git_mode", "archive_mode", "size_bytes"},
                "semver inventory entry fields must be closed")
        require(metadata["git_mode"] in ("100644", "100755") and
                metadata["archive_mode"] in ("0644", "0755") and
                metadata["archive_mode"] == ("0755" if metadata["git_mode"] == "100755" else "0644"),
                "semver inventory mode mismatch")
        require(type(metadata["size_bytes"]) is int and metadata["size_bytes"] >= 0 and
                metadata["size_bytes"] <= EXPANDED_LIMIT and
                isinstance(metadata["sha256"], str) and len(metadata["sha256"]) == 64 and
                isinstance(metadata["git_blob"], str) and len(metadata["git_blob"]) == 40,
                "semver inventory digest or size invalid")
        result[normalized] = metadata
    return result


def _tree_hash(inventory):
    directories = {"": {}}
    for path, metadata in inventory.items():
        parts = path.split("/")
        parent = ""
        for part in parts[:-1]:
            child = part if not parent else parent + "/" + part
            current = directories.setdefault(parent, {})
            prior = current.get(part)
            require(prior in (None, (True, None)), "semver tree has file/directory collision")
            current[part] = (True, None)
            directories.setdefault(child, {})
            parent = child
        current = directories.setdefault(parent, {})
        name = parts[-1]
        require(name not in current, "semver tree has duplicate path")
        current[name] = (False, metadata["git_blob"])

    hashes = {}
    for directory in sorted(directories, key=lambda value: (value == "", -value.count("/"), value)):
        raw = bytearray()
        children = directories[directory]
        for name, (is_directory, object_id) in sorted(
                children.items(), key=lambda pair: (pair[0] + ("/" if pair[1][0] else "")).encode("utf-8")):
            child = name if not directory else directory + "/" + name
            mode = b"40000" if is_directory else inventory[child]["git_mode"].encode()
            child_id = hashes[child] if is_directory else object_id
            raw.extend(mode + b" " + name.encode("utf-8") + b"\0" + bytes.fromhex(child_id))
        hashes[directory] = git_object("tree", bytes(raw))
    return hashes[""]


def _git_files_match(inventory, git_files):
    if git_files is None:
        return
    require(set(git_files) == set(inventory), "semver archive inventory differs from Git files")
    for name, metadata in inventory.items():
        mode, object_id = git_files[name]
        require(mode.decode() == metadata["git_mode"] and object_id == metadata["git_blob"],
                "semver inventory Git object differs")


def _entry_payload(archive, entry, mode):
    require(entry.linkname == "" and not getattr(entry, "sparse", None),
            "semver archive links and sparse files are forbidden")
    require(entry.size >= 0 and entry.size <= EXPANDED_LIMIT, "semver archive size is invalid")
    if mode == "dir":
        require(entry.isdir() and entry.size == 0 and entry.mode == 0o755,
                "semver directory mode or type mismatch")
        return b""
    require(entry.isreg() and entry.mode in (0o644, 0o755),
            "semver file type or mode mismatch")
    stream = archive.extractfile(entry)
    require(stream is not None, "semver archive file payload missing")
    payload = stream.read(EXPANDED_LIMIT + 1)
    require(len(payload) == entry.size, "semver archive file size changed")
    return payload


def archive_proof(data, inventory, revision, git_files=None):
    """Return exact source members after checking the finite tar contract."""
    require(len(data) <= ARCHIVE_LIMIT, "semver source archive exceeds bound")
    expected = _inventory(inventory, revision)
    _git_files_match(expected, git_files)
    files, directories, aliases = {}, set(), set()
    expanded = 0
    try:
        archive = tarfile.open(fileobj=io.BytesIO(data), mode="r:")
    except (tarfile.TarError, OSError) as error:
        raise ValueError("semver source archive is not a readable tar") from error
    with archive:
        require(archive.format == tarfile.PAX_FORMAT and archive.pax_headers ==
                {"comment": EXPECTED_PAX}, "semver archive global PAX differs")
        for index, entry in enumerate(archive):
            require(index < ENTRY_LIMIT, "semver archive entry limit exceeded")
            require(entry.pax_headers == {"comment": EXPECTED_PAX},
                    "semver archive contains unknown PAX fields")
            name = portable_path(entry.name)
            key = unicodedata.normalize("NFKC", name).casefold()
            require(key not in aliases, "semver archive has duplicate or aliased path")
            aliases.add(key)
            if entry.isdir():
                require(name not in directories and name not in files, "semver duplicate directory")
                _entry_payload(archive, entry, "dir")
                directories.add(name)
                continue
            require(name not in files and name not in directories and name in expected,
                    "semver archive file set differs")
            payload = _entry_payload(archive, entry, "file")
            metadata = expected[name]
            require(entry.mode == int(metadata["archive_mode"], 8) and
                    len(payload) == metadata["size_bytes"] and sha(payload) == metadata["sha256"] and
                    git_object("blob", payload) == metadata["git_blob"],
                    "semver archive member proof differs")
            files[name] = payload
            expanded += len(payload)
            require(expanded <= EXPANDED_LIMIT, "semver archive expanded size exceeded")
    required_dirs = set()
    for name in files:
        parent = posixpath.dirname(name)
        while parent:
            required_dirs.add(parent)
            parent = posixpath.dirname(parent)
    require(directories == required_dirs, "semver archive directory closure differs")
    require(set(files) == set(expected) and _tree_hash(expected) == revision.source_tree,
            "semver source tree reconstruction differs")
    return files
