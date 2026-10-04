"""Closed native operations retain lifetime, source lease and original frame.

Only the fixed native owner executes commands. Paths and returned JSON select or
describe existing leases; they never issue execution or native-frame authority.
"""
import json

from source_intent_native_semantics_validation import (
    NativeSourceSemanticsUnavailable, _require, _validate_result,
)
from source_semantics_edited import (
    _seal_edited_snapshot, _begin_lock_transition, _complete_lock_transition,
)
from source_semantics_leases import _load_snapshot_registry, SourceInvocationLease
from source_semantics_lifetime import load_source_sdk_lifetime
from source_semantics_manifest_read import _capture_manifest_read, _unique_object, ManifestReadWitness

_SDKS, _BOUND_SDKS = {}, {}
_COMPILED_NATIVE_LOCK_COMPLETION_TYPE = None
_MAX_RESPONSE = 32 * 1024 * 1024


def _decode(raw):
    _require(type(raw) is bytes and 0 < len(raw) <= _MAX_RESPONSE,
             'source_native_response_bound')
    try:
        return json.loads(raw, object_pairs_hook=_unique_object,
                          parse_constant=lambda _value: _require(
                              False, 'source_native_nonfinite_json'))
    except (ValueError, UnicodeError, RecursionError) as error:
        raise NativeSourceSemanticsUnavailable('source_native_response_invalid') from error


class SourceSemanticsSdk:
    __slots__ = ('_lifetime', '_registry')

    def __init__(self):
        _require(False, 'source_sdk_private_factory_required')

    def __setattr__(self, _name, _value):
        _require(False, 'source_sdk_immutable')

    def require_current(self):
        record = _SDKS.get(id(self))
        _require(record is not None and record[0] is self and
                 record[1] is self._lifetime and record[2] is self._registry and
                 self._registry._lifetime is self._lifetime,
                 'source_sdk_expired_or_foreign')
        self._lifetime.require_current()
        self._registry.require_current()
        for entry in self._registry._snapshots:
            entry.require_current()
        self._lifetime.require_current()

    def cargo_identity(self):
        try:
            self.require_current()
            raw = self._lifetime.native_executor().cargo_identity()
            _require(type(raw) is bytes and 0 < len(raw) <= 64 * 1024,
                     'source_cargo_identity_observation_bound')
            raw.decode('utf-8')
            self.require_current()
            return raw
        except BaseException:
            self.close()
            raise

    def _bind_native_request(self, manifest, operation, governing=None):
        self.require_current()
        original = None
        if operation == 'workspace_version_lock_mutation':
            _require(_COMPILED_NATIVE_LOCK_COMPLETION_TYPE is not None,
                     'source_native_lock_completion_owner_unavailable')
            matches = [entry for entry in self._registry._snapshots
                       if manifest in entry._manifests]
            _require(len(matches) == 1 and matches[0]._edited is not None,
                     'source_mutation_sealed_edited_manifest_required')
            matches[0]._edited.require_current()
            original = matches[0]._edited._original
        invocation = self._registry._select(manifest, operation, governing, original)
        sdk = object.__new__(SourceOperationSdk)
        object.__setattr__(sdk, '_parent', self)
        object.__setattr__(sdk, '_invocation', invocation)
        _BOUND_SDKS[id(sdk)] = (sdk, self, invocation)
        sdk.require_current()
        return sdk

    def _admit_native_edited_scratch(self, witness):
        self.require_current()
        _require(type(witness) is ManifestReadWitness and
                 witness._invocation._registry is self._registry,
                 'source_edited_foreign_sdk_witness')
        result = _seal_edited_snapshot(witness)
        self.require_current()
        return result

    def close(self):
        record = _SDKS.get(id(self))
        _require(record is not None and record[0] is self,
                 'source_sdk_expired_or_foreign')
        _SDKS.pop(id(self))
        for key, bound_record in tuple(_BOUND_SDKS.items()):
            if bound_record[1] is self:
                _BOUND_SDKS.pop(key)
        _close_all(record[2], record[1])

    def __reduce_ex__(self, _protocol):
        _require(False, 'source_sdk_not_serializable')


class SourceOperationSdk:
    __slots__ = ('_parent', '_invocation')

    def __init__(self):
        _require(False, 'source_operation_private_selection_required')

    def __setattr__(self, _name, _value):
        _require(False, 'source_operation_immutable')

    def require_current(self):
        record = _BOUND_SDKS.get(id(self))
        _require(record is not None and record[0] is self and
                 record[1] is self._parent and record[2] is self._invocation,
                 'source_operation_expired_or_foreign')
        self._parent.require_current()
        self._invocation.require_current()

    def source_operand(self):
        self.require_current()
        return self._invocation

    def require_source_operand(self, operand):
        self.require_current()
        _require(type(operand) is SourceInvocationLease and operand is self._invocation,
                 'source_operation_foreign_operand')

    def manifest_read(self, operand):
        try:
            self._require_operation(operand, 'manifest_read', None)
            witness = operand._snapshot._witnesses.get(operand._manifest)
            if witness is None:
                witness = _capture_manifest_read(operand)
            _require(type(witness) is ManifestReadWitness,
                     'source_original_witness_exact_owner_required')
            witness.require_current()
            _require(witness._invocation._snapshot is operand._snapshot and
                     witness._invocation._manifest == operand._manifest and
                     witness._lifetime is self._parent._lifetime,
                     'source_original_witness_operand_binding_changed')
            self.require_current()
            return _decode(witness.observation_bytes())
        except BaseException:
            _close_issued_parent(self)
            raise

    def full_locked_metadata(self, operand, governing):
        try:
            self._require_operation(operand, 'full_locked_metadata', governing)
            executor = self._parent._lifetime.native_executor()
            result = _decode(executor.full_locked_metadata(operand))
            before = executor.governing_context_observation(operand)
            paths = {name: self._parent._lifetime._sdk.installed_tool(name).path
                     for name in ('cargo', 'rustc')}
            _validate_result(result, 'full_locked_metadata', before, paths)
            self.require_current()
            return result
        except BaseException:
            _close_issued_parent(self)
            raise

    def comparison_read(self, operand):
        try:
            self._require_operation(operand, 'comparison_read', None)
            executor = self._parent._lifetime.native_executor()
            raw = executor.comparison_read(operand)
            executor.require_comparison_observation(raw, operand)
            self.require_current()
            return _decode(raw)
        except BaseException:
            _close_issued_parent(self)
            raise

    def workspace_version_lock_mutation(self, operand, governing):
        transition = None
        try:
            self._require_operation(operand, 'workspace_version_lock_mutation', governing)
            _require(_COMPILED_NATIVE_LOCK_COMPLETION_TYPE is not None,
                     'source_native_lock_completion_owner_unavailable')
            executor = self._parent._lifetime.native_executor()
            transition = _begin_lock_transition(operand)
            completion = executor.workspace_version_lock_mutation(transition, operand._original._frame)
            _require(type(completion) is _COMPILED_NATIVE_LOCK_COMPLETION_TYPE,
                     'source_native_foreign_lock_completion')
            updated = _complete_lock_transition(transition, completion)
            updated.require_current()
            self._parent.require_current()
            return _decode(completion.response_bytes)
        except BaseException:
            _close_issued_parent(self)
            raise
        finally:
            # Mutation retires this invocation even on failure. Its old OriginalFS
            # state cannot be used to authenticate a changed governing lock.
            _BOUND_SDKS.pop(id(self), None)
            if transition is not None:
                transition._revoke()

    def _require_operation(self, operand, operation, governing):
        self.require_source_operand(operand)
        _require(operand._operation == operation and governing is operand._governing,
                 'source_operation_purpose_or_context_substituted')

    def __reduce_ex__(self, _protocol):
        _require(False, 'source_operation_not_serializable')


def _close_issued_parent(operation):
    record = _BOUND_SDKS.get(id(operation))
    _require(record is not None and record[0] is operation,
             'source_bound_sdk_expired_or_foreign')
    record[1].close()


def _close_all(registry, lifetime):
    errors = []
    for lease in (registry, lifetime):
        if lease is not None:
            try:
                lease.close()
            except BaseException as error:
                errors.append(error)
    if errors:
        raise NativeSourceSemanticsUnavailable('source_sdk_cleanup_failed') from errors[0]


def load_source_semantics_sdk():
    """One source-owned factory; no descriptors, paths or callbacks are inputs."""
    lifetime, registry = None, None
    try:
        lifetime = load_source_sdk_lifetime()
        registry = _load_snapshot_registry(lifetime)
        sdk = object.__new__(SourceSemanticsSdk)
        object.__setattr__(sdk, '_lifetime', lifetime)
        object.__setattr__(sdk, '_registry', registry)
        _SDKS[id(sdk)] = (sdk, lifetime, registry)
        sdk.require_current()
        return sdk
    except BaseException:
        if 'sdk' in locals():
            _SDKS.pop(id(sdk), None)
        _close_all(registry, lifetime)
        raise
