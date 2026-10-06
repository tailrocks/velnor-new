"""Only the snapshot writer can admit native edited scratch and reseal changes."""
from source_intent_native_semantics_validation import _require
from source_semantics_leases import (
    _admit_snapshot, _REGISTRY_RECORDS, _INVOCATIONS, _INVOCATION_RECORDS, _revoke_entry,
)
from source_semantics_manifest_read import ManifestReadWitness

_EDITED = {}
_TRANSITIONS = {}
_EDITED_RECORDS, _TRANSITION_RECORDS = {}, {}
_COMPILED_NATIVE_LOCK_TRANSITION_TYPE = None


class EditedLease:
    __slots__ = ('_registry', '_entry', '_original', '_writer', '_manifest')

    def __init__(self):
        _require(False, 'source_edited_private_writer_required')

    def __setattr__(self, _name, _value):
        _require(False, 'source_edited_immutable')

    def require_current(self):
        _require(_EDITED.get(id(self)) is self, 'source_edited_expired_or_foreign')
        record = _EDITED_RECORDS.get(id(self))
        _require(record is not None and all(actual is held for actual, held in zip(
            (self._registry, self._entry, self._original, self._writer), record[:4])) and
            type(self._manifest) is str and self._manifest == record[4],
            'source_edited_held_binding_changed')
        self._original.require_current()
        self._entry.require_current()
        _require(self._entry._registry is self._registry and
                 self._entry._edited is self and self._registry._owner is self._writer,
                 'source_edited_owner_changed')
        self._writer.require_edited_snapshot(
            self._entry._acquired, self._original._invocation._snapshot._acquired,
            self._original._frame)
        _require(self._entry._acquired.derived_manifest_path(
            self._original._invocation._manifest) == self._manifest and
            self._manifest in self._entry._manifests,
            'source_edited_original_manifest_binding_changed')
        self._original.require_current()

    def _revoke(self):
        _EDITED.pop(id(self), None)
        _EDITED_RECORDS.pop(id(self), None)

    def __reduce_ex__(self, _protocol):
        _require(False, 'source_edited_not_serializable')


def _require_disjoint_original(acquired, original):
    _require(type(acquired.root) is str and acquired.root != original._state.root and
             not acquired.root.startswith(original._state.root + '/') and
             not original._state.root.startswith(acquired.root + '/'),
             'source_edited_original_tree_must_stay_read_only')


def _seal_edited_snapshot(witness):
    _require(type(witness) is ManifestReadWitness, 'source_edited_original_witness_required')
    witness.require_current()
    registry = witness._invocation._registry
    writer = registry._owner
    original = witness._invocation._snapshot
    acquired = writer.acquire_edited_snapshot(original._acquired, witness._frame)
    writer.require_acquired_snapshot(acquired)
    writer.require_edited_snapshot(acquired, original._acquired, witness._frame)
    _require_disjoint_original(acquired, original)
    entry = _admit_snapshot(registry, acquired)
    try:
        manifest = acquired.derived_manifest_path(witness._invocation._manifest)
        _require(manifest in entry._manifests, 'source_edited_manifest_not_admitted')
        edited = object.__new__(EditedLease)
        for name, value in (('_registry', registry), ('_entry', entry),
                            ('_original', witness), ('_writer', writer), ('_manifest', manifest)):
            object.__setattr__(edited, name, value)
        _EDITED[id(edited)] = edited
        _EDITED_RECORDS[id(edited)] = (registry, entry, witness, writer, manifest)
        object.__setattr__(entry, '_edited', edited)
        edited.require_current()
        return edited
    except BaseException:
        _retire_entry(registry, entry)
        raise


def _retire_entry(registry, entry):
    """Revocation precedes descriptor cleanup and source-owner resealing."""
    record = _REGISTRY_RECORDS[id(registry)]
    _revoke_entry(entry)
    for invocation in record[7]:
        held = _INVOCATION_RECORDS.get(id(invocation))
        if held is not None and held[3] is entry:
            _INVOCATIONS.pop(id(invocation), None)
            _INVOCATION_RECORDS.pop(id(invocation), None)
    record[6][:] = [item for item in record[6] if item is not entry]
    object.__setattr__(registry, '_snapshots', tuple(record[6]))


class LockMutationLease:
    __slots__ = ('_registry', '_invocation', '_edited', '_pending', '_before')

    def __init__(self):
        _require(False, 'source_lock_transition_private_writer_required')

    def __setattr__(self, _name, _value):
        _require(False, 'source_lock_transition_immutable')

    def require_current(self):
        _require(_TRANSITIONS.get(id(self)) is self,
                 'source_lock_transition_expired_or_foreign')
        record = _TRANSITION_RECORDS.get(id(self))
        _require(record is not None and all(actual is held for actual, held in zip(
            (self._registry, self._invocation, self._edited, self._pending, self._before), record)),
            'source_lock_transition_held_binding_changed')
        self._registry.require_current()
        self._edited._original.require_current()
        self._registry._owner.require_pending_lock_transition(
            self._pending, self._invocation, self._edited._original._frame)

    def _revoke(self):
        _TRANSITIONS.pop(id(self), None)
        _TRANSITION_RECORDS.pop(id(self), None)

    def __reduce_ex__(self, _protocol):
        _require(False, 'source_lock_transition_not_serializable')


def _begin_lock_transition(invocation):
    invocation.require_current()
    entry, registry = invocation._snapshot, invocation._registry
    edited = entry._edited
    _require(_COMPILED_NATIVE_LOCK_TRANSITION_TYPE is not None and edited is not None and
             invocation._operation == 'workspace_version_lock_mutation' and
             invocation._original is edited._original,
             'source_lock_transition_requires_edited_original_lease')
    before = entry._state.manifest_bytes
    # This fixed owner operation is data-only: retain native operands and revoke
    # the acquisition. No write or process may precede that revocation.
    pending = registry._owner.begin_governing_lock_transition(
        entry._acquired, invocation, edited._original._frame)
    _require(type(pending) is _COMPILED_NATIVE_LOCK_TRANSITION_TYPE,
             'source_lock_transition_foreign_native_owner')
    _retire_entry(registry, entry)
    transition = object.__new__(LockMutationLease)
    for name, value in (('_registry', registry), ('_invocation', invocation),
                        ('_edited', edited), ('_pending', pending), ('_before', before)):
        object.__setattr__(transition, name, value)
    _TRANSITIONS[id(transition)] = transition
    _TRANSITION_RECORDS[id(transition)] = (registry, invocation, edited, pending, before)
    transition.require_current()
    return transition


def _complete_lock_transition(transition, native_completion):
    """A native completion handle is validated by its issuing executor, never JSON."""
    _require(type(transition) is LockMutationLease and
             _TRANSITIONS.get(id(transition)) is transition,
             'source_lock_completion_requires_pending_transition')
    transition.require_current()
    registry, invocation, edited = transition._registry, transition._invocation, transition._edited
    registry._lifetime.require_current()
    executor = registry._lifetime.native_executor()
    executor.require_mutation_completion(native_completion, transition, edited._original._frame)
    acquired = registry._owner.complete_governing_lock_transition(
        transition._pending, native_completion, transition._before, edited._original._frame)
    registry._owner.require_lock_successor(acquired, transition._pending, native_completion)
    _require_disjoint_original(acquired, edited._original._invocation._snapshot)
    successor = _admit_snapshot(registry, acquired)
    updated = object.__new__(EditedLease)
    for name, value in (('_registry', registry), ('_entry', successor),
                        ('_original', edited._original), ('_writer', registry._owner),
                        ('_manifest', edited._manifest)):
        object.__setattr__(updated, name, value)
    _EDITED[id(updated)] = updated
    _EDITED_RECORDS[id(updated)] = (
        registry, successor, edited._original, registry._owner, edited._manifest)
    object.__setattr__(successor, '_edited', updated)
    updated.require_current()
    transition._revoke()
    return updated
