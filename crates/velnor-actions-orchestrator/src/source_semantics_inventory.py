"""Private OriginalFS observation state for one admitted source materialization."""

import os
import stat

from source_archive_inventory_common import (
    MAX_ENTRIES,
    MAX_MANIFEST,
    MAX_PATH_BYTES,
    InventoryError,
    absolute,
    relative,
)
from source_archive_inventory_fs import root_descriptor
from source_archive_inventory_original import _original_inventory


_STATE_SEAL = object()


def _reject(reason):
    raise InventoryError("source_semantics_" + reason)


def _canonical_root(root):
    try:
        value = os.fspath(root)
    except TypeError as error:
        raise InventoryError("source_semantics_root_invalid") from error
    if type(value) is not str:
        _reject("root_type")
    try:
        absolute(value)
        encoded = os.fsencode(value)
    except (InventoryError, TypeError, UnicodeError) as error:
        raise InventoryError("source_semantics_root_invalid") from error
    if len(encoded) > MAX_PATH_BYTES or os.path.realpath(value) != value:
        _reject("root_alias")
    return value


def _identity(info):
    if not stat.S_ISDIR(info.st_mode):
        _reject("root_kind")
    return info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid


def _top_names(descriptor):
    names = []
    try:
        with os.scandir(descriptor) as entries:
            for entry in entries:
                if type(entry.name) is not str:
                    _reject("top_name_type")
                relative(entry.name)
                names.append(entry.name)
                if len(names) > MAX_ENTRIES:
                    _reject("top_entry_limit")
    except (InventoryError, OSError, TypeError, UnicodeError, ValueError) as error:
        if isinstance(error, InventoryError):
            raise
        raise InventoryError("source_semantics_top_unavailable") from error
    names.sort()
    return tuple(names)


def _open_current(root, expected):
    descriptor = None
    try:
        descriptor = root_descriptor(root)
        if _identity(os.fstat(descriptor)) != expected:
            _reject("root_changed")
        result = descriptor
        descriptor = None
        return result
    except (InventoryError, OSError, ValueError) as error:
        if descriptor is not None:
            os.close(descriptor)
        raise InventoryError("source_semantics_root_changed") from error


def _close(descriptor):
    if descriptor is not None:
        os.close(descriptor)


class _OriginalSourceState:
    """Read-only observation state; it cannot issue source or execution authority."""

    __slots__ = ('_root', '_descriptor', '_identity', '_roots', '_manifest', '_seal')

    def __init__(self, root, borrowed_descriptor, *, _seal=None):
        if _seal is not _STATE_SEAL or type(borrowed_descriptor) is not int:
            _reject("state_authority")
        root = _canonical_root(root)
        try:
            descriptor = os.dup(borrowed_descriptor)
            identity = _identity(os.fstat(descriptor))
            roots = _top_names(descriptor)
            current = _open_current(root, identity)
            try:
                if _top_names(current) != roots:
                    _reject("topology_changed")
            finally:
                os.close(current)
            result = _capture(root, roots, descriptor)
            current = _open_current(root, identity)
            try:
                if _top_names(current) != roots:
                    _reject("topology_changed")
            finally:
                os.close(current)
        except InventoryError:
            if 'descriptor' in locals():
                _close(descriptor)
            raise
        except (OSError, RecursionError) as error:
            if 'descriptor' in locals():
                _close(descriptor)
            raise InventoryError("source_semantics_capture_unavailable") from error
        object.__setattr__(self, '_root', root)
        object.__setattr__(self, '_descriptor', descriptor)
        object.__setattr__(self, '_identity', identity)
        object.__setattr__(self, '_roots', roots)
        object.__setattr__(self, '_manifest', result.canonical_bytes)
        object.__setattr__(self, '_seal', _seal)

    def __setattr__(self, _name, _value):
        raise InventoryError("source_semantics_state_immutable")

    def require_current(self):
        if getattr(self, '_seal', None) is not _STATE_SEAL or self._descriptor is None:
            _reject("state_closed")
        try:
            if _identity(os.fstat(self._descriptor)) != self._identity:
                _reject("root_changed")
        except OSError as error:
            raise InventoryError("source_semantics_root_changed") from error
        current = _open_current(self._root, self._identity)
        try:
            if _top_names(current) != self._roots:
                _reject("topology_changed")
        finally:
            os.close(current)
        result = _capture(self._root, self._roots, self._descriptor)
        current = _open_current(self._root, self._identity)
        try:
            if _top_names(current) != self._roots:
                _reject("topology_changed")
        finally:
            os.close(current)
        if result.canonical_bytes != self._manifest:
            _reject("manifest_changed")

    @property
    def root(self):
        self.require_current()
        return self._root

    @property
    def manifest_bytes(self):
        self.require_current()
        return self._manifest

    def close(self):
        descriptor = self._descriptor
        object.__setattr__(self, '_descriptor', None)
        if descriptor is not None:
            os.close(descriptor)


def _capture(root, roots, descriptor):
    if len(roots) > MAX_ENTRIES or len(os.fsencode(root)) > MAX_PATH_BYTES:
        _reject("inventory_limit")
    try:
        borrowed = os.dup(descriptor)
    except OSError as error:
        raise InventoryError("source_semantics_capture_unavailable") from error
    try:
        result = _original_inventory(root, roots, borrowed)
    finally:
        os.close(borrowed)
    if len(result.canonical_bytes) > MAX_MANIFEST:
        _reject("manifest_limit")
    return result


def _capture_original_source(root, borrowed_descriptor):
    """Capture a source-owned root; the caller retains and closes borrowed_descriptor."""
    return _OriginalSourceState(root, borrowed_descriptor, _seal=_STATE_SEAL)
