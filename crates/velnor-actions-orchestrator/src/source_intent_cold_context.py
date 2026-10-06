"""Private fresh-install Foundation context for observation, never cache export."""
import os
from types import MappingProxyType

from source_intent_cold_common import ColdSourceIntent
from source_archive_inventory_fs import root_descriptor
from source_intent_cold_recipe import _LEAVES, _digest

_COMPILED_COLD_FOUNDATION_PROFILE = None
_CONTEXT_SEAL = object()


def _foundation_profile(witness):
    profile = _COMPILED_COLD_FOUNDATION_PROFILE
    fields = {'schema', 'purpose', 'host', 'runtime_abi', 'qualification_sha256',
              'installer_source_identity', 'mise_qualification_sha256'}
    if (type(profile) is not dict or set(profile) != fields or type(profile['schema']) is not int
            or profile['schema'] != 1 or profile['purpose'] != 'source-intent-cold-sdk-foundation'
            or profile['host'] != 'x86_64-unknown-linux-gnu'
            or profile['runtime_abi'] != 'source-original-fs-v3'
            or not _digest(profile['qualification_sha256'])
            or profile['installer_source_identity'] != witness.installer_source_identity
            or profile['mise_qualification_sha256'] != witness.mise_qualification_sha256):
        raise ColdSourceIntent('cold_sdk_foundation_qualification_unavailable')
    return dict(profile)


class FreshColdInstallationContext:
    """Rooted descriptor bound to actual fixed-source successful installation."""
    __slots__ = ('_witness', '_descriptor', '_identity', '_profile', '_seal')

    def __init__(self, witness, *, _seal=None):
        from source_intent_cold_install import FreshColdInstallationWitness
        if _seal is not _CONTEXT_SEAL or type(witness) is not FreshColdInstallationWitness:
            raise ColdSourceIntent('cold_sdk_foundation_context_authority')
        witness.require_current()
        profile = _foundation_profile(witness)
        descriptor = root_descriptor(witness.root)
        object.__setattr__(self, '_witness', witness)
        object.__setattr__(self, '_descriptor', descriptor)
        object.__setattr__(self, '_identity', _identity(os.fstat(descriptor)))
        object.__setattr__(self, '_profile', MappingProxyType(profile))
        object.__setattr__(self, '_seal', _seal)

    def __setattr__(self, _name, _value):
        raise ColdSourceIntent('cold_sdk_foundation_context_immutable')

    def require_current(self):
        if getattr(self, '_seal', None) is not _CONTEXT_SEAL or self._descriptor is None:
            raise ColdSourceIntent('cold_sdk_foundation_context_authority')
        self._witness.require_current()
        if dict(self._profile) != _foundation_profile(self._witness):
            raise ColdSourceIntent('cold_sdk_foundation_profile_changed')
        current = root_descriptor(self._witness.root)
        try:
            if (_identity(os.fstat(current)) != self._identity
                    or _identity(os.fstat(self._descriptor)) != self._identity):
                raise ColdSourceIntent('cold_sdk_foundation_root_changed')
        finally:
            os.close(current)

    @property
    def root_descriptor(self):
        self.require_current()
        return os.dup(self._descriptor)

    @property
    def root(self):
        self.require_current()
        return self._witness.root

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


def _from_completed_installation(witness):
    return FreshColdInstallationContext(witness, _seal=_CONTEXT_SEAL)
