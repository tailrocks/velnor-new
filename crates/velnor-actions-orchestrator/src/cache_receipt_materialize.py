"""Private numbered-quarantine relocation into the source-owned namespace.

The source helper must capture this capability before observing cache bytes.
Unpublished source bindings and shared projection capabilities select cold.
"""
import os
import stat

from cache_receipt import VerifiedQuarantinePayload
from cache_receipt_common import ColdReceipt, _absolute
from cache_receipt_manifest import _root_descriptor, _stable
from cache_receipt_virtual import _numbered, _signed_entries
from cache_receipt_materialize_transaction import MaterializationStop
from opaque_inventory_metadata import metadata_records
from source_archive_inventory_common import InventoryError
from source_archive_inventory_fs import read_descriptor

_NAMESPACE_AUTHORITY = object()
_COMPILED_SOURCE_BINDING = None
_COMPILED_PROJECTION = None
_COMPILED_PROJECTION_TYPE = None
_DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW


class _CanonicalNamespace:
    __slots__ = ("_descriptor", "_path", "_policy", "_projection", "_identity", "_seal")

    def __init__(self, authority, descriptor, path, policy, projection):
        if authority is not _NAMESPACE_AUTHORITY:
            raise ColdReceipt("materialize_namespace_authority")
        for name, value in (("_descriptor", descriptor), ("_path", path),
                            ("_policy", policy), ("_projection", projection),
                            ("_identity", _directory_identity(os.fstat(descriptor))),
                            ("_seal", authority)):
            object.__setattr__(self, name, value)

    def __setattr__(self, _name, _value):
        raise ColdReceipt("materialize_namespace_immutable")

    def close(self):
        if self._descriptor is not None:
            os.close(self._descriptor)
            object.__setattr__(self, "_descriptor", None)

    def require(self, grant):
        if (getattr(self, "_seal", None) is not _NAMESPACE_AUTHORITY
                or self._descriptor is None or self._policy is not grant._policy
                or self._policy is not _COMPILED_SOURCE_BINDING
                or self._projection is not _COMPILED_PROJECTION):
            raise ColdReceipt("materialize_namespace_authority")
        self._policy.require_qualified()
        _require_projection(self._projection, self._policy)
        try:
            descriptor = _root_descriptor(self._path)
            try:
                if _directory_identity(os.fstat(descriptor)) != self._identity:
                    raise ColdReceipt("materialize_namespace_changed")
                _owned_directory(descriptor, self._path)
            finally:
                os.close(descriptor)
        except (ColdReceipt, OSError) as error:
            raise MaterializationStop("materialize_namespace_unsafe") from error


def _directory_identity(info):
    return info.st_dev, info.st_ino


def _owned_directory(descriptor, path=None):
    info = os.fstat(descriptor)
    if info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise ColdReceipt("materialize_directory_owner")
    if path is not None:
        try:
            metadata_records(path, info, "reject")
        except ValueError as error:
            raise ColdReceipt("materialize_directory_metadata") from error


def _require_projection(projection, policy):
    if _COMPILED_PROJECTION_TYPE is None or type(projection) is not _COMPILED_PROJECTION_TYPE:
        raise ColdReceipt("materialize_projection_authority")
    projection.require(policy)


def _capture_source_namespace():
    """Fixed RUNNER_TEMP/velnor capability; no caller destination argument."""
    policy, projection = _COMPILED_SOURCE_BINDING, _COMPILED_PROJECTION
    if policy is None or projection is None:
        raise ColdReceipt("materialize_source_unqualified")
    policy.require_qualified()
    _require_projection(projection, policy)
    runner_temp = os.environ.get("RUNNER_TEMP", "")
    _absolute(runner_temp)
    if os.environ.get("VELNOR_CACHE_PAYLOAD_ROOT") != runner_temp + "/velnor":
        raise ColdReceipt("materialize_namespace_binding")
    parent = _root_descriptor(runner_temp)
    try:
        if os.fstat(parent).st_uid != os.geteuid():
            raise ColdReceipt("materialize_runner_temp_owner")
        try:
            os.mkdir("velnor", 0o700, dir_fd=parent)
        except FileExistsError:
            pass
        descriptor = os.open("velnor", _DIRECTORY_FLAGS, dir_fd=parent)
    finally:
        os.close(parent)
    try:
        _owned_directory(descriptor, runner_temp + "/velnor")
        return _CanonicalNamespace(_NAMESPACE_AUTHORITY, descriptor,
                                   runner_temp + "/velnor", policy, projection)
    except BaseException:
        os.close(descriptor)
        raise


def _parent_descriptor(boundary, path, create=False, root_path=None):
    descriptor = os.dup(boundary)
    physical = root_path
    try:
        for component in path.split("/")[:-1]:
            if create:
                try:
                    os.mkdir(component, 0o700, dir_fd=descriptor)
                except FileExistsError:
                    pass
            child = os.open(component, _DIRECTORY_FLAGS, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
            if create:
                physical = os.path.join(physical, component)
                _owned_directory(descriptor, physical)
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def _require_empty_roots(namespace, roots):
    for root in roots:
        try:
            descriptor = _parent_descriptor(namespace._descriptor, root)
        except FileNotFoundError:
            continue
        try:
            try:
                os.stat(root.split("/")[-1], dir_fd=descriptor, follow_symlinks=False)
            except FileNotFoundError:
                continue
            raise ColdReceipt("materialize_destination_present")
        finally:
            os.close(descriptor)


def _copy_file(source_parent, destination_parent, source_name, name, entry):
    source = os.open(source_name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                     dir_fd=source_parent)
    destination = None
    try:
        before = os.fstat(source)
        if not stat.S_ISREG(before.st_mode) or stat.S_IMODE(before.st_mode) != entry["mode"]:
            raise ColdReceipt("materialize_source_file_changed")
        destination = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                              0o600, dir_fd=destination_parent)
        def consume(chunk):
            remaining = memoryview(chunk)
            while remaining:
                written = os.write(destination, remaining)
                if written <= 0:
                    raise ColdReceipt("materialize_write")
                remaining = remaining[written:]

        try:
            digest, _size = read_descriptor(source, before, True, consume)
        except InventoryError as error:
            code = ("materialize_byte_limit" if str(error) == "payload_byte_limit"
                    else "materialize_source_file_changed")
            raise ColdReceipt(code) from error
        if digest != entry["sha256"] or _stable(os.fstat(source)) != _stable(before):
            raise ColdReceipt("materialize_source_file_changed")
        os.fchmod(destination, entry["mode"])
        os.fsync(destination)
    finally:
        if destination is not None:
            os.close(destination)
        os.close(source)


def _copy_entry(stage, namespace, roots, entry):
    path, kind = entry["path"], entry["kind"]
    if kind == "missing":
        return
    destination = _parent_descriptor(namespace._descriptor, path, create=True,
                                     root_path=namespace._path)
    try:
        name = path.split("/")[-1]
        if kind == "directory":
            os.mkdir(name, 0o700, dir_fd=destination)
        elif kind == "symlink":
            # Authenticated original logical target, never numbered link text.
            os.symlink(entry["target"], name, dir_fd=destination)
        elif kind == "file":
            physical = _numbered(path, roots)
            source = _parent_descriptor(stage, physical)
            try:
                _copy_file(source, destination, physical.split("/")[-1], name, entry)
            finally:
                os.close(source)
        else:
            raise ColdReceipt("materialize_entry_kind")
    finally:
        os.close(destination)


def _finish_directories(grant, namespace, entries):
    for entry in sorted(entries, key=lambda item: item["path"].count("/"), reverse=True):
        if entry["kind"] != "directory":
            continue
        grant.require_current()
        namespace.require(grant)
        parent = _parent_descriptor(namespace._descriptor, entry["path"])
        try:
            descriptor = os.open(entry["path"].split("/")[-1], _DIRECTORY_FLAGS, dir_fd=parent)
            try:
                os.fchmod(descriptor, entry["mode"])
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
        finally:
            os.close(parent)


def _materialize_verified(grant, namespace):
    """Source helper only: verify before writes and prove exact live projection."""
    if type(grant) is not VerifiedQuarantinePayload or type(namespace) is not _CanonicalNamespace:
        raise ColdReceipt("materialize_authority")
    grant.require_current()
    namespace.require(grant)
    _require_empty_roots(namespace, grant.logical_roots)
    from cache_receipt_materialize_transaction import _Assembly
    assembly = _Assembly(namespace)
    try:
        _assemble_verified(grant, assembly.namespace)
        grant.require_current()
        namespace.require(grant)
        assembly.commit(grant)
        namespace.require(grant)
        actual = namespace._projection.inventory_live(namespace._path, grant._policy)
        if actual != grant.manifest_bytes:
            raise ColdReceipt("materialize_live_inventory_mismatch")
        grant.require_current()
        namespace.require(grant)
        os.fsync(namespace._descriptor)
        assembly.close()
        return grant.manifest_sha256
    except BaseException:
        try:
            assembly.rollback()
            assembly.close()
        except BaseException:
            assembly.abandon_descriptors()
            raise
        raise


def _assemble_verified(grant, namespace):
    entries = tuple(_signed_entries(grant.manifest_bytes).values())
    stage = _root_descriptor(grant.quarantine)
    try:
        _owned_directory(stage, grant.quarantine)
        for entry in sorted(entries, key=lambda item: (item["path"].count("/"), item["path"])):
            grant.require_current()
            namespace.require(grant)
            current_stage = _root_descriptor(grant.quarantine)
            try:
                if _directory_identity(os.fstat(current_stage)) != _directory_identity(os.fstat(stage)):
                    raise ColdReceipt("materialize_quarantine_changed")
            finally:
                os.close(current_stage)
            _copy_entry(stage, namespace, grant.logical_roots, entry)
        grant.require_current()
        _finish_directories(grant, namespace, entries)
        namespace.require(grant)
        actual = namespace._projection.inventory_live(namespace._path, grant._policy)
        if actual != grant.manifest_bytes:
            raise ColdReceipt("materialize_live_inventory_mismatch")
        grant.require_current()
        namespace.require(grant)
        os.fsync(namespace._descriptor)
    except (OSError, RecursionError) as error:
        raise ColdReceipt("materialize_unavailable") from error
    finally:
        os.close(stage)
