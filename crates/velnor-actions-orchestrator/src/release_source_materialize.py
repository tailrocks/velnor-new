"""Write validated pure content into a fresh private tree; execute nothing."""
import hashlib
import os
from pathlib import Path
import stat
import unicodedata


def _materialize_blob(snapshot, entry):
    raw = snapshot.blobs.get(entry["sha"])
    require(type(raw) is bytes and len(raw) == entry["size"], "source_materialize_blob_size")
    actual = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
    require(actual == entry["sha"], "source_materialize_blob_digest")
    return raw


def _materialize_preflight(snapshot):
    require(type(snapshot) is _SourceSnapshot and snapshot._seal is _SNAPSHOT_SEAL,
            "source_snapshot_content")
    entries = _source_entries({"sha": snapshot.tree_sha, "truncated": False,
                               "tree": [{"path": path, **entry}
                                        for path, entry in snapshot.entries.items()]},
                              snapshot.tree_sha)
    _source_verify_trees(entries, snapshot.tree_sha)
    require(sum(entry.get("size", 0) for entry in entries.values()) <= _SOURCE_MAX_TOTAL,
            "source_materialize_total_size")
    names = {}
    for path, entry in entries.items():
        parts = path.split("/")
        require(all(len(part.encode("utf-8")) <= 255 for part in parts),
                "source_materialize_component_size")
        key = "/".join(unicodedata.normalize("NFC", part).casefold() for part in parts)
        require(key not in names, "source_materialize_host_collision")
        names[key] = path
        if entry["type"] == "blob":
            _materialize_blob(snapshot, entry)
    expected = {entry["sha"] for entry in entries.values() if entry["type"] == "blob"}
    require(set(snapshot.blobs) == expected, "source_materialize_blob_coverage")
    return entries


def _materialize_open_directory(start, parts, identities=None):
    descriptor = os.dup(start)
    traversed = []
    try:
        for part in parts:
            traversed.append(part)
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
            if identities is not None:
                require(_materialize_identity(os.fstat(descriptor)) ==
                        identities.get("/".join(traversed)), "source_materialize_directory_ownership")
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def _materialize_parent(destination):
    require(type(destination) in (str, Path) or isinstance(destination, Path),
            "source_materialize_destination")
    raw = os.fspath(destination)
    _source_path(raw[1:] if raw.startswith("/") else raw)
    path = Path(destination)
    require(path.name not in ("", ".", "..") and ".." not in path.parts,
            "source_materialize_destination")
    _source_path(path.name)
    require(len(path.name.encode("utf-8")) <= 255, "source_materialize_destination")
    path = path.absolute()
    root = os.open(path.anchor, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        parent = _materialize_open_directory(root, path.parent.parts[1:])
    finally:
        os.close(root)
    info = os.fstat(parent)
    if info.st_uid != os.getuid() or info.st_mode & 0o022:
        os.close(parent)
        raise ReconcileError("source_materialize_parent_private")
    return path, parent


def _materialize_identity(info):
    return info.st_dev, info.st_ino, stat.S_IFMT(info.st_mode)


def _materialize_write(root, path, entry, snapshot, owned):
    parts = path.split("/")
    parent = _materialize_open_directory(root, parts[:-1], owned)
    try:
        if entry["type"] == "tree":
            os.mkdir(parts[-1], 0o700, dir_fd=parent)
            owned[path] = _materialize_identity(
                os.stat(parts[-1], dir_fd=parent, follow_symlinks=False))
            return
        raw = _materialize_blob(snapshot, entry)
        descriptor = os.open(parts[-1], os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                             0o600, dir_fd=parent)
        try:
            owned[path] = _materialize_identity(os.fstat(descriptor))
            view = memoryview(raw)
            while view:
                count = os.write(descriptor, view)
                require(count > 0, "source_materialize_short_write")
                view = view[count:]
            os.fchmod(descriptor, 0o755 if entry["mode"] == "100755" else 0o644)
        finally:
            os.close(descriptor)
    finally:
        os.close(parent)


def _materialize_cleanup(root, owned):
    for path, identity in reversed(owned.items()):
        parts = path.split("/")
        parent = _materialize_open_directory(root, parts[:-1], owned)
        try:
            actual = _materialize_identity(os.stat(parts[-1], dir_fd=parent,
                                                   follow_symlinks=False))
            require(actual == identity, "source_materialize_cleanup_ownership")
            if identity[2] == stat.S_IFDIR:
                os.rmdir(parts[-1], dir_fd=parent)
            else:
                os.unlink(parts[-1], dir_fd=parent)
        finally:
            os.close(parent)


def _materialize_source_snapshot_owned(snapshot, destination):
    """Retain the original write descriptors; this result grants no authority."""
    entries = _materialize_preflight(snapshot)
    path, parent = _materialize_parent(destination)
    root, identity, owned, retained = None, None, {}, False
    try:
        os.mkdir(path.name, 0o700, dir_fd=parent)
        identity = _materialize_identity(os.stat(path.name, dir_fd=parent, follow_symlinks=False))
        root = os.open(path.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
        require(_materialize_identity(os.fstat(root)) == identity,
                "source_materialize_root_ownership")
        for name, entry in sorted(entries.items(), key=lambda item: (item[0].count("/"), item[0])):
            _materialize_write(root, name, entry, snapshot, owned)
        result = _MaterializedSource(path, parent, root, identity, owned)
        result.require_current()
        retained = True
        return result
    except BaseException:
        if identity is not None:
            try:
                if root is not None:
                    _materialize_cleanup(root, owned)
                require(_materialize_identity(os.stat(path.name, dir_fd=parent,
                                                       follow_symlinks=False)) == identity,
                        "source_materialize_cleanup_ownership")
                os.rmdir(path.name, dir_fd=parent)
            except (OSError, ReconcileError) as error:
                raise ReconcileError("source_materialize_cleanup") from error
        raise
    finally:
        if not retained:
            if root is not None:
                os.close(root)
            os.close(parent)


class _MaterializedSource:
    """Private owned write result, retaining the same parent and root descriptors."""
    __slots__ = ('path', '_parent', '_root', '_identity', '_parent_identity', '_owned')

    def __init__(self, path, parent, root, identity, owned):
        self.path, self._parent, self._root = path, parent, root
        self._identity = identity
        self._parent_identity = _materialize_identity(os.fstat(parent))
        self._owned = dict(owned)

    def require_current(self):
        require(self._root is not None and self._parent is not None,
                'source_materialize_result_closed')
        require(_materialize_identity(os.fstat(self._root)) == self._identity and
                _materialize_identity(os.fstat(self._parent)) == self._parent_identity and
                _materialize_identity(os.stat(self.path.name, dir_fd=self._parent,
                                             follow_symlinks=False)) == self._identity,
                'source_materialize_result_identity')
        _, current = _materialize_parent(self.path)
        try:
            require(_materialize_identity(os.fstat(current)) == self._parent_identity,
                    'source_materialize_result_parent')
        finally:
            os.close(current)

    @property
    def root_descriptor(self):
        self.require_current()
        return os.dup(self._root)

    def close(self):
        errors = []
        for name in ('_root', '_parent'):
            descriptor = getattr(self, name)
            setattr(self, name, None)
            if descriptor is not None:
                try:
                    os.close(descriptor)
                except OSError as error:
                    errors.append(error)
        if errors:
            raise ReconcileError('source_materialize_result_close') from errors[0]

    def abort(self):
        try:
            self.require_current()
            _materialize_cleanup(self._root, self._owned)
            require(_materialize_identity(os.stat(self.path.name, dir_fd=self._parent,
                                                 follow_symlinks=False)) == self._identity,
                    'source_materialize_cleanup_ownership')
            os.rmdir(self.path.name, dir_fd=self._parent)
        finally:
            self.close()


def materialize_source_snapshot(snapshot, destination):
    """Materialize pure content and close its owned descriptors; execute nothing."""
    result = _materialize_source_snapshot_owned(snapshot, destination)
    try:
        return result.path
    finally:
        result.close()
