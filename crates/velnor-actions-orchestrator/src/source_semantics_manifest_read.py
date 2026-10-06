"""Original native summaries stay inside their genuine native owner frame.

Response bytes are observations. No decoder can create the retained native frame.
"""
import json

from source_intent_native_semantics_validation import (
    NativeSourceSemanticsUnavailable, _require, _validate_result,
)
from source_semantics_leases import SourceInvocationLease

_COMPILED_NATIVE_MANIFEST_FRAME_TYPE = None
_WITNESSES = {}
_WITNESS_RECORDS = {}
_MAX_RESPONSE = 32 * 1024 * 1024


class ManifestReadWitness:
    __slots__ = ('_invocation', '_lifetime', '_executor', '_frame', '_frame_type',
                 '_response')

    def __init__(self):
        raise NativeSourceSemanticsUnavailable('source_original_native_frame_required')

    def __setattr__(self, _name, _value):
        raise NativeSourceSemanticsUnavailable('source_original_witness_immutable')

    def require_current(self):
        _require(_WITNESSES.get(id(self)) is self,
                 'source_original_witness_expired_or_foreign')
        record = _WITNESS_RECORDS.get(id(self))
        _require(record is not None and all(actual is held for actual, held in zip(
            (self._invocation, self._lifetime, self._executor, self._frame,
             self._frame_type, self._response), record)),
            'source_original_witness_held_binding_changed')
        self._invocation.require_current()
        self._lifetime.require_current()
        _require(self._lifetime.native_executor() is self._executor and
                 self._frame_type is _COMPILED_NATIVE_MANIFEST_FRAME_TYPE and
                 type(self._frame) is self._frame_type and
                 self._invocation._snapshot._witnesses.get(self._invocation._manifest) is self,
                 'source_original_witness_owner_changed')
        self._executor.require_manifest_read_frame(self._frame, self._invocation)
        _require(self._frame.response_bytes == self._response,
                 'source_original_native_observation_changed')
        self._invocation.require_current()

    def observation_bytes(self):
        self.require_current()
        return self._response

    def _revoke(self):
        _WITNESSES.pop(id(self), None)
        _WITNESS_RECORDS.pop(id(self), None)

    def __reduce_ex__(self, _protocol):
        raise NativeSourceSemanticsUnavailable('source_original_witness_not_serializable')


def _response(raw):
    _require(type(raw) is bytes and 0 < len(raw) <= _MAX_RESPONSE,
             'source_original_native_response_bound')
    try:
        result = json.loads(raw, object_pairs_hook=_unique_object,
                            parse_constant=lambda _value: _require(
                                False, 'source_original_native_nonfinite_json'))
        _require(type(result) is dict and type(result.get('native_context')) is dict and
                 'before' in result['native_context'], 'source_original_native_context')
        before = result['native_context']['before']
        _validate_result(result, 'manifest_read', before, {})
        _require(result['compiler_context'] == {'kind': 'not_invoked'},
                 'source_original_manifest_read_invoked_compiler')
        return None
    except (ValueError, TypeError, UnicodeError, RecursionError) as error:
        raise NativeSourceSemanticsUnavailable('source_original_native_response_invalid') from error


def _unique_object(pairs):
    result = {}
    for name, value in pairs:
        _require(name not in result, 'source_original_native_duplicate_json_key')
        result[name] = value
    return result


def _capture_manifest_read(invocation):
    _require(type(invocation) is SourceInvocationLease and
             _COMPILED_NATIVE_MANIFEST_FRAME_TYPE is not None,
             'source_original_native_frame_owner_unavailable')
    invocation.require_current()
    _require(invocation._operation == 'manifest_read' and
             invocation._governing is None and invocation._original is None and
             invocation._manifest not in invocation._snapshot._witnesses,
             'source_original_manifest_read_phase')
    registry = invocation._registry
    registry._owner.require_original_snapshot(invocation._snapshot._acquired)
    lifetime = registry._lifetime
    executor = lifetime.native_executor()
    frame = executor.manifest_read(invocation)
    try:
        _require(type(frame) is _COMPILED_NATIVE_MANIFEST_FRAME_TYPE,
                 'source_original_foreign_native_frame')
        executor.require_manifest_read_frame(frame, invocation)
        raw = frame.response_bytes
        _response(raw)
        invocation.require_current()
        lifetime.require_current()
        witness = object.__new__(ManifestReadWitness)
        values = {'_invocation': invocation, '_lifetime': lifetime, '_executor': executor,
                  '_frame': frame, '_frame_type': type(frame), '_response': raw}
        for name, value in values.items():
            object.__setattr__(witness, name, value)
        _WITNESSES[id(witness)] = witness
        _WITNESS_RECORDS[id(witness)] = (
            invocation, lifetime, executor, frame, type(frame), raw)
        invocation._snapshot._witnesses[invocation._manifest] = witness
        witness.require_current()
        return witness
    except BaseException:
        if 'witness' in locals():
            witness._revoke()
        executor.release_manifest_read_frame(frame)
        raise
