"""Closed lifetime lease for the source-owned semantic SDK."""
from source_intent_cold_common import ColdSourceIntent
from source_intent_cold_sdk import ColdSourceIntentSdk


# The immutable source composer replaces these slots with one exact owner.
_COMPILED_SOURCE_SEMANTICS_OWNER_TYPE = None
_COMPILED_SOURCE_SEMANTICS_OWNER_GETTER = None
_COMPILED_SOURCE_SEMANTICS_EXECUTOR_TYPE = None

_LIFETIME_SEAL = object()
_ISSUING = object()
_LIVE_LIFETIME = None
_LIVE_REGISTRY = {}


def _reject(reason):
    raise ColdSourceIntent('source_sdk_lifetime_' + reason)


def _require_callable(value, reason):
    if not callable(value):
        _reject(reason)


def _close_owner(owner):
    closer = getattr(owner, 'close', None)
    if not callable(closer):
        _reject('owner_close')
    closer()


def _discard_owner(owner):
    try:
        _close_owner(owner)
    except BaseException:
        pass


def _binding():
    owner_type = _COMPILED_SOURCE_SEMANTICS_OWNER_TYPE
    getter = _COMPILED_SOURCE_SEMANTICS_OWNER_GETTER
    executor_type = _COMPILED_SOURCE_SEMANTICS_EXECUTOR_TYPE
    if (not isinstance(owner_type, type) or not callable(getter)
            or not isinstance(executor_type, type)):
        _reject('issuer_unavailable')
    return owner_type, getter, executor_type


def _owner_methods(owner, owner_type):
    if type(owner) is not owner_type:
        _reject('owner_origin')
    role = getattr(owner, 'compiled_role', None)
    if type(role) is not str or role != 'SourceSemantics':
        _reject('owner_role')
    for name in ('require_current', 'compiler_sdk', 'native_executor', 'close'):
        _require_callable(getattr(owner, name, None), 'owner_' + name)


def _issue(owner_type, getter, executor_type):
    owner = getter()
    try:
        _owner_methods(owner, owner_type)
        owner.require_current()
        sdk = owner.compiler_sdk()
        if type(sdk) is not ColdSourceIntentSdk:
            _reject('sdk_origin')
        sdk.require_current()
        executor = owner.native_executor()
        if type(executor) is not executor_type:
            _reject('executor_origin')
        _require_callable(getattr(executor, 'require_current', None), 'executor_current')
        executor.require_current()
        return SourceSdkLifetime(owner, sdk, executor, owner_type, getter, executor_type,
                                 _seal=_LIFETIME_SEAL)
    except BaseException:
        _discard_owner(owner)
        raise


class SourceSdkLifetime:
    """One immutable lease retaining the exact owner, SDK and executor."""
    __slots__ = ('_owner', '_sdk', '_executor', '_owner_type', '_owner_getter',
                 '_executor_type', '_seal')

    def __init__(self, owner, sdk, executor, owner_type, owner_getter, executor_type,
                 *, _seal=None):
        if _seal is not _LIFETIME_SEAL:
            _reject('authority')
        for name, value in (('_owner', owner), ('_sdk', sdk), ('_executor', executor),
                            ('_owner_type', owner_type), ('_owner_getter', owner_getter),
                            ('_executor_type', executor_type), ('_seal', _seal)):
            object.__setattr__(self, name, value)

    def __setattr__(self, _name, _value):
        _reject('immutable')

    def _entry(self):
        entry = _LIVE_REGISTRY.get(id(self))
        if entry is None or entry[0] is not self:
            _reject('authority')
        return entry

    def require_current(self):
        entry = self._entry()
        if (getattr(self, '_seal', None) is not _LIFETIME_SEAL
                or entry[1] is not self._owner or entry[2] is not self._sdk
                or entry[3] is not self._executor):
            _reject('authority')
        owner_type, getter, executor_type = _binding()
        if (owner_type is not self._owner_type or getter is not self._owner_getter
                or executor_type is not self._executor_type
                or type(self._owner) is not owner_type
                or type(self._executor) is not executor_type):
            _reject('binding_changed')
        _owner_methods(self._owner, owner_type)
        if self._owner_getter() is not self._owner:
            _reject('owner_changed')
        self._owner.require_current()
        if (type(self._owner.compiled_role) is not str
                or self._owner.compiled_role != 'SourceSemantics'):
            _reject('owner_role')
        sdk = self._owner.compiler_sdk()
        if type(sdk) is not ColdSourceIntentSdk or sdk is not self._sdk:
            _reject('sdk_changed')
        sdk.require_current()
        executor = self._owner.native_executor()
        if type(executor) is not executor_type or executor is not self._executor:
            _reject('executor_changed')
        _require_callable(getattr(executor, 'require_current', None), 'executor_current')
        executor.require_current()

    def native_executor(self):
        self.require_current()
        return self._executor

    def close(self):
        if _LIVE_LIFETIME is not self:
            _reject('authority')
        entry = _LIVE_REGISTRY.get(id(self))
        if entry is None or entry[0] is not self:
            _reject('authority')
        _LIVE_REGISTRY.pop(id(self), None)
        _close_owner(entry[1])

    def __copy__(self):
        _reject('copy')

    def __deepcopy__(self, _memo):
        _reject('copy')

    def __reduce__(self):
        _reject('copy')

    def __reduce_ex__(self, _protocol):
        _reject('copy')


def load_source_sdk_lifetime():
    """Issue the one source-owned semantic SDK lifetime, with no caller inputs."""
    global _LIVE_LIFETIME
    if _LIVE_LIFETIME is not None or _LIVE_REGISTRY:
        _reject('already_issued')
    _LIVE_LIFETIME = _ISSUING
    try:
        owner_type, getter, executor_type = _binding()
        lifetime = _issue(owner_type, getter, executor_type)
        _LIVE_LIFETIME = lifetime
        try:
            _LIVE_REGISTRY[id(lifetime)] = (lifetime, lifetime._owner,
                                            lifetime._sdk, lifetime._executor)
            lifetime.require_current()
        except BaseException:
            _LIVE_REGISTRY.pop(id(lifetime), None)
            _discard_owner(lifetime._owner)
            raise
        return lifetime
    except BaseException:
        if _LIVE_LIFETIME is _ISSUING:
            _LIVE_LIFETIME = None
        raise
