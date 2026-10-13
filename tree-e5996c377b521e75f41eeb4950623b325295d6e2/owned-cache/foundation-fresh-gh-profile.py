"""Private initial-Node SDK capsule; observation files never issue profiles."""
import os

from cache_receipt_common import ColdReceipt
from foundation_fresh_gh_checker import require_current as _check_original

_PRIVATE_FOUNDATION_SEED = None
_ISSUER = object()


class _Foundation:
    __slots__ = ('_seed', 'control_root', 'payload_roots', 'ca_file')

    def __init__(self, token, seed):
        if token is not _ISSUER:
            raise ColdReceipt('foundation_private_issuer')
        object.__setattr__(self, '_seed', seed)
        object.__setattr__(self, 'control_root', seed['control_root'])
        object.__setattr__(self, 'payload_roots', tuple(seed['payload_roots']))
        object.__setattr__(self, 'ca_file', seed['ca_file'])

    def __setattr__(self, _name, _value):
        raise ColdReceipt('foundation_profile_immutable')

    def require_current(self):
        # This is the sole Foundation owner's checker compiled from the private
        # original Node-held SDK record, never a readback observation document.
        _check_original(self._seed)


def current():
    seed = _PRIVATE_FOUNDATION_SEED
    if seed is None:
        raise ColdReceipt('foundation_original_record_unissued')
    if (os.uname().sysname != 'Linux' or seed['record']['initial_trust'] !=
            'reviewed-immutable-first-step-fresh-runner-sdk'):
        raise ColdReceipt('foundation_initial_boundary')
    profile = _Foundation(_ISSUER, seed)
    profile.require_current()
    return profile
