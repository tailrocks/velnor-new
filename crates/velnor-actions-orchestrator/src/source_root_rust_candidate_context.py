"""Private rooted context for one completed RootRust candidate installation."""

import os

from source_archive_inventory_common import InventoryError
from source_archive_inventory_fs import root_descriptor
from source_intent_cold_common import ColdSourceIntent
from source_root_rust_candidate_recipe import _LEAVES


_CONTEXT_SEAL = object()


class FreshRootRustCandidateContext:
    """Hold a sealed witness and descriptor for observation only."""

    __slots__ = ('_witness', '_root', '_descriptor', '_identity', '_seal')

    def __init__(self, witness, *, _seal=None):
        from source_root_rust_candidate_install import RootRustCandidateInstallationWitness

        if (_seal is not _CONTEXT_SEAL
                or type(witness) is not RootRustCandidateInstallationWitness):
            raise ColdSourceIntent('root_candidate_context_authority')
        witness.require_current()
        root = witness.root
        try:
            descriptor = root_descriptor(root)
        except (InventoryError, OSError) as error:
            raise ColdSourceIntent('root_candidate_context_root_unavailable') from error
        try:
            identity = _identity(os.fstat(descriptor))
        except OSError as error:
            os.close(descriptor)
            raise ColdSourceIntent('root_candidate_context_root_unavailable') from error
        object.__setattr__(self, '_witness', witness)
        object.__setattr__(self, '_root', root)
        object.__setattr__(self, '_descriptor', descriptor)
        object.__setattr__(self, '_identity', identity)
        object.__setattr__(self, '_seal', _seal)

    def __setattr__(self, _name, _value):
        raise ColdSourceIntent('root_candidate_context_immutable')

    def require_current(self):
        if getattr(self, '_seal', None) is not _CONTEXT_SEAL or self._descriptor is None:
            raise ColdSourceIntent('root_candidate_context_authority')
        self._witness.require_current()
        root = self._witness.root
        if root != self._root:
            raise ColdSourceIntent('root_candidate_context_root_changed')
        try:
            current = root_descriptor(root)
            try:
                if (_identity(os.fstat(current)) != self._identity
                        or _identity(os.fstat(self._descriptor)) != self._identity):
                    raise ColdSourceIntent('root_candidate_context_root_changed')
            finally:
                os.close(current)
        except (InventoryError, OSError) as error:
            raise ColdSourceIntent('root_candidate_context_root_changed') from error

    @property
    def root_descriptor(self):
        self.require_current()
        return os.dup(self._descriptor)

    @property
    def root(self):
        self.require_current()
        return self._root

    @property
    def roots(self):
        self.require_current()
        return _LEAVES

    def close(self):
        if self._descriptor is not None:
            os.close(self._descriptor)
            object.__setattr__(self, '_descriptor', None)


def _identity(info):
    return info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid


def _from_completed_candidate(witness):
    return FreshRootRustCandidateContext(witness, _seal=_CONTEXT_SEAL)
