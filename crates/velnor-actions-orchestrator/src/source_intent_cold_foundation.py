"""Same-process adapters to sole Foundation source-owner private issuers."""
from source_intent_cold_common import ColdSourceIntent

# Bound only inside the separately reviewed, fixed source-owner capsule.
# A production SourceIntent issuer is unavailable. Candidate cannot substitute.
_COMPILED_SOURCE_INTENT_FOUNDATION_TYPE = None
_COMPILED_SOURCE_INTENT_FOUNDATION_GETTER = None
_COMPILED_ROOT_RUST_CANDIDATE_FOUNDATION_TYPE = None
_COMPILED_ROOT_RUST_CANDIDATE_FOUNDATION_GETTER = None
_FOUNDATION_SEAL = object()


def _binding(purpose):
    if purpose == 'source-intent-cold-sdk':
        return (_COMPILED_SOURCE_INTENT_FOUNDATION_TYPE,
                _COMPILED_SOURCE_INTENT_FOUNDATION_GETTER)
    if purpose == 'root-linux-compiler-candidate':
        return (_COMPILED_ROOT_RUST_CANDIDATE_FOUNDATION_TYPE,
                _COMPILED_ROOT_RUST_CANDIDATE_FOUNDATION_GETTER)
    raise ColdSourceIntent('cold_foundation_purpose')


class FreshPreparationFoundation:
    """Retains the exact issuer object; observations never reconstruct it."""
    __slots__ = ('_owner', '_purpose', '_type', '_getter', '_seal')

    def __init__(self, owner, purpose, owner_type, getter, *, _seal=None):
        if _seal is not _FOUNDATION_SEAL or type(owner) is not owner_type:
            raise ColdSourceIntent('cold_foundation_issuer_authority')
        for name, value in (('_owner', owner), ('_purpose', purpose), ('_type', owner_type),
                            ('_getter', getter), ('_seal', _seal)):
            object.__setattr__(self, name, value)
        self.require_current()

    def __setattr__(self, _name, _value):
        raise ColdSourceIntent('cold_foundation_immutable')

    def require_current(self):
        if getattr(self, '_seal', None) is not _FOUNDATION_SEAL:
            raise ColdSourceIntent('cold_foundation_issuer_authority')
        owner_type, getter = _binding(self._purpose)
        if (owner_type is not self._type or getter is not self._getter
                or type(self._owner) is not owner_type):
            raise ColdSourceIntent('cold_foundation_issuer_changed')
        self._owner.require_current()
        if (self._purpose == 'root-linux-compiler-candidate'
                and self._owner.compiled_role != 'RootRustCandidate'):
            raise ColdSourceIntent('cold_foundation_foreign_role')

    def require_purpose(self, purpose):
        self.require_current()
        if purpose != self._purpose:
            raise ColdSourceIntent('cold_foundation_foreign_purpose')

    @property
    def candidate_root(self):
        self.require_purpose('root-linux-compiler-candidate')
        return self._owner.candidate_root

    @property
    def ca_file(self):
        self.require_purpose('root-linux-compiler-candidate')
        return self._owner.ca_file

    def require_candidate_root(self, root):
        self.require_purpose('root-linux-compiler-candidate')
        if self._owner.candidate_root != root:
            raise ColdSourceIntent('cold_foundation_candidate_root')

    def require_acquired_native(self):
        self.require_purpose('root-linux-compiler-candidate')
        validator = getattr(self._owner, 'require_acquired_native', None)
        if not callable(validator):
            raise ColdSourceIntent('cold_foundation_acquired_native_unavailable')
        validator()
        self.require_current()

    def require_compiler_source_native(self):
        self.require_purpose('root-linux-compiler-candidate')
        validator = getattr(self._owner, 'require_compiler_source_native', None)
        if not callable(validator):
            raise ColdSourceIntent('cold_foundation_compiler_source_native_unavailable')
        validator()
        self.require_current()

    def require_installed_native(self):
        self.require_purpose('root-linux-compiler-candidate')
        validator = getattr(self._owner, 'require_installed_native', None)
        if not callable(validator):
            raise ColdSourceIntent('cold_foundation_installed_native_unavailable')
        validator()
        self.require_current()


def require_preparation_foundation(purpose):
    owner_type, getter = _binding(purpose)
    if not isinstance(owner_type, type) or not callable(getter):
        raise ColdSourceIntent('cold_foundation_issuer_unavailable')
    # Sole source-owned getter runs once; subsequent checks retain this instance.
    return FreshPreparationFoundation(getter(), purpose, owner_type, getter,
                                      _seal=_FOUNDATION_SEAL)
