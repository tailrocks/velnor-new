"""Private preparation registry. Requested paths select already admitted source.

The compiled acquisition owner must authenticate and materialize its snapshots.
Neither OriginalFS observations nor this registry authenticate caller paths.
"""
import os
from source_intent_native_semantics_validation import NativeSourceSemanticsUnavailable
from source_semantics_inventory import _capture_original_source
from source_semantics_lifetime import SourceSdkLifetime

_COMPILED_PREPARATION_OWNER_TYPE = None
_COMPILED_PREPARATION_OWNER_GETTER = None
_COMPILED_ACQUIRED_SNAPSHOT_TYPE = None
_COMPILED_NATIVE_GOVERNING_CONTEXT_TYPE = None
_REGISTRIES, _ENTRIES, _INVOCATIONS, _USED_LIFETIMES = {}, {}, {}, {}
_ENTRY_RECORDS, _INVOCATION_RECORDS, _REGISTRY_RECORDS = {}, {}, {}
_OPERATIONS = frozenset(('manifest_read', 'comparison_read',
                         'full_locked_metadata', 'workspace_version_lock_mutation'))


def _require(condition, reason):
    if not condition:
        raise NativeSourceSemanticsUnavailable(reason)


def _member(value, instances, reason):
    _require(instances.get(id(value)) is value, reason)


class SnapshotLease:
    __slots__ = ('_registry', '_acquired', '_state', '_manifests', '_witnesses', '_edited')

    def __init__(self):
        raise NativeSourceSemanticsUnavailable('source_snapshot_private_acquisition_required')

    def __setattr__(self, _name, _value):
        raise NativeSourceSemanticsUnavailable('source_snapshot_immutable')

    def require_current(self):
        _member(self, _ENTRIES, 'source_snapshot_expired_or_foreign')
        record = _ENTRY_RECORDS.get(id(self))
        _require(record is not None and all(actual is held for actual, held in zip(
            (self._registry, self._acquired, self._state, self._manifests, self._witnesses),
            record)), 'source_snapshot_held_binding_changed')
        _require_entry_attachments(self)
        self._registry.require_current()
        self._registry._owner.require_acquired_snapshot(self._acquired)
        self._acquired.require_current()
        self._state.require_current()
        _require(self._acquired.root == self._state.root and
                 self._acquired.manifest_paths == self._manifests,
                 'source_snapshot_manifest_admission_changed')
        self._acquired.require_current()
        self._registry.require_current()

    def source_operand(self):
        self.require_current()
        return self


def _require_entry_attachments(entry):
    from source_semantics_edited import EditedLease, _EDITED, _EDITED_RECORDS
    from source_semantics_manifest_read import _WITNESSES, _WITNESS_RECORDS
    edited = [value for key, value in _EDITED.items()
              if _EDITED_RECORDS.get(key, (None, None))[1] is entry]
    _require((not edited and entry._edited is None) or
             (len(edited) == 1 and type(entry._edited) is EditedLease and
              entry._edited is edited[0]), 'source_snapshot_edited_binding_changed')
    expected = {}
    for key, held in _WITNESS_RECORDS.items():
        invocation = _INVOCATION_RECORDS.get(id(held[0]))
        if invocation is not None and invocation[3] is entry:
            expected[invocation[0]] = _WITNESSES.get(key)
    _require(len(expected) == len(entry._witnesses) and all(
        entry._witnesses.get(path) is witness and witness is not None
        for path, witness in expected.items()), 'source_snapshot_witness_binding_changed')


class SourceInvocationLease:
    __slots__ = ('_registry', '_snapshot', '_manifest', '_identity', '_operation',
                 '_governing', '_original')

    def __init__(self):
        raise NativeSourceSemanticsUnavailable('source_invocation_private_selection_required')

    def __setattr__(self, _name, _value):
        raise NativeSourceSemanticsUnavailable('source_invocation_immutable')

    def require_current(self):
        _member(self, _INVOCATIONS, 'source_invocation_expired_or_foreign')
        record = _INVOCATION_RECORDS.get(id(self))
        _require(record is not None and type(self._manifest) is str and
                 type(self._operation) is str and
                 (self._manifest, self._operation) == record[:2] and
                 all(actual is held for actual, held in zip(
                     (self._registry, self._snapshot, self._identity,
                      self._governing, self._original), record[2:])),
                 'source_invocation_held_binding_changed')
        self._snapshot.require_current()
        _require(self._snapshot._registry is self._registry and
                 self._manifest in self._snapshot._manifests and
                 self._snapshot._acquired.require_manifest(self._manifest) is self._identity,
                 'source_invocation_manifest_identity_changed')
        if self._governing is not None:
            self._registry._owner.require_governing_context(self._governing, self._snapshot)
        if self._original is not None:
            self._original.require_current()
        if self._operation == 'workspace_version_lock_mutation':
            _require(self._snapshot._edited is not None and
                     self._snapshot._edited._original is self._original,
                     'source_invocation_sealed_edit_required')
            self._snapshot._edited.require_current()
        self._registry._owner.require_operation(
            self._snapshot._acquired, self._operation, self._governing, self._original)

    def source_operand(self):
        self.require_current()
        return self


class SnapshotLeaseRegistry:
    __slots__ = ('_owner', '_owner_type', '_owner_getter', '_snapshot_type',
                 '_lifetime', '_snapshots', '_invocations')

    def __init__(self):
        raise NativeSourceSemanticsUnavailable('source_registry_private_preparation_required')

    def __setattr__(self, _name, _value):
        raise NativeSourceSemanticsUnavailable('source_registry_immutable')

    def require_current(self):
        _member(self, _REGISTRIES, 'source_registry_expired_or_foreign')
        record = _REGISTRY_RECORDS.get(id(self))
        _require(record is not None and all(actual is held for actual, held in zip(
            (self._owner, self._owner_type, self._owner_getter, self._snapshot_type,
             self._lifetime, self._invocations), record[:6])) and
            type(self._snapshots) is tuple and len(self._snapshots) == len(record[6]) and
            all(actual is held for actual, held in zip(self._snapshots, record[6])) and
            len(self._invocations) == len(record[7]) and
            all(actual is held for actual, held in zip(self._invocations, record[7])),
            'source_registry_held_binding_changed')
        _require(self._owner_type is _COMPILED_PREPARATION_OWNER_TYPE and
                 self._owner_getter is _COMPILED_PREPARATION_OWNER_GETTER and
                 self._snapshot_type is _COMPILED_ACQUIRED_SNAPSHOT_TYPE,
                 'source_registry_compiled_owner_changed')
        _require(self._owner_getter() is self._owner, 'source_registry_owner_replaced')
        self._owner.require_current()
        self._lifetime.require_current()

    def _select(self, manifest, operation, governing=None, original=None):
        self.require_current()
        _require(type(manifest) is str and type(operation) is str and
                 operation in _OPERATIONS, 'source_invocation_request_shape')
        if operation in ('manifest_read', 'comparison_read'):
            _require(governing is None and original is None,
                     'source_invocation_read_context_must_be_absent')
        else:
            _require(_COMPILED_NATIVE_GOVERNING_CONTEXT_TYPE is not None and
                     type(governing) is _COMPILED_NATIVE_GOVERNING_CONTEXT_TYPE,
                     'source_invocation_native_governing_context_required')
        matches = []
        for entry in self._snapshots:
            entry.require_current()
            if manifest in entry._manifests:
                matches.append(entry)
        _require(len(matches) == 1, 'source_invocation_unadmitted_or_ambiguous_manifest')
        entry = matches[0]
        self._owner.require_operation(entry._acquired, operation, governing, original)
        if governing is not None:
            self._owner.require_governing_context(governing, entry)
        if original is not None:
            original.require_current()
        values = {'_registry': self, '_snapshot': entry, '_manifest': manifest,
                  '_identity': entry._acquired.require_manifest(manifest),
                  '_operation': operation, '_governing': governing, '_original': original}
        invocation = object.__new__(SourceInvocationLease)
        for name, value in values.items():
            object.__setattr__(invocation, name, value)
        _INVOCATIONS[id(invocation)] = invocation
        _INVOCATION_RECORDS[id(invocation)] = (
            manifest, operation, self, entry, invocation._identity, governing, original)
        self._invocations.append(invocation)
        _REGISTRY_RECORDS[id(self)][7].append(invocation)
        invocation.require_current()
        return invocation

    def close(self):
        if _REGISTRIES.get(id(self)) is not self:
            return
        _REGISTRIES.pop(id(self))
        record = _REGISTRY_RECORDS.pop(id(self))
        errors = []
        for entry in tuple(record[6]):
            try:
                _revoke_entry(entry)
            except BaseException as error:
                errors.append(error)
        for invocation in record[7]:
            _INVOCATIONS.pop(id(invocation), None)
            _INVOCATION_RECORDS.pop(id(invocation), None)
        from source_semantics_edited import _TRANSITIONS, _TRANSITION_RECORDS
        for key, held in tuple(_TRANSITION_RECORDS.items()):
            if held[0] is self:
                _TRANSITIONS.pop(key, None)
                _TRANSITION_RECORDS.pop(key, None)
        if errors:
            raise NativeSourceSemanticsUnavailable('source_registry_cleanup_failed') from errors[0]


def _attach_entry(registry, entry):
    roster = _REGISTRY_RECORDS[id(registry)][6]
    roster.append(entry)
    object.__setattr__(registry, '_snapshots', tuple(roster))


def _revoke_entry(entry):
    record = _ENTRY_RECORDS.pop(id(entry), None)
    _ENTRIES.pop(id(entry), None)
    if record is None:
        return
    from source_semantics_edited import _EDITED, _EDITED_RECORDS
    from source_semantics_manifest_read import _WITNESSES, _WITNESS_RECORDS
    for key, held in tuple(_EDITED_RECORDS.items()):
        if held[1] is entry:
            _EDITED.pop(key, None)
            _EDITED_RECORDS.pop(key, None)
    for key, held in tuple(_WITNESS_RECORDS.items()):
        invocation_record = _INVOCATION_RECORDS.get(id(held[0]))
        if invocation_record is not None and invocation_record[3] is entry:
            _WITNESSES.pop(key, None)
            _WITNESS_RECORDS.pop(key, None)
    errors = []
    for witness in tuple(record[4].values()):
        try:
            witness._revoke()
        except BaseException as error:
            errors.append(error)
    try:
        record[2].close()
    except BaseException as error:
        errors.append(error)
    if errors:
        raise NativeSourceSemanticsUnavailable('source_snapshot_cleanup_failed') from errors[0]


def _admit_snapshot(registry, acquired):
    _require(type(acquired) is registry._snapshot_type,
             'source_snapshot_foreign_acquisition')
    registry._owner.require_acquired_snapshot(acquired)
    acquired.require_current()
    paths = acquired.manifest_paths
    _require(type(paths) is tuple and 0 < len(paths) <= 20000 and
             all(type(path) is str for path in paths) and len(set(paths)) == len(paths),
             'source_snapshot_manifest_admission')
    root = acquired.root
    _require(type(root) is str and all(type(path) is str and
             path.startswith(root + '/') and '\x00' not in path and
             all(part not in ('', '.', '..') for part in path[1:].split('/'))
             for path in paths), 'source_snapshot_manifest_containment')
    descriptor = acquired.root_descriptor
    try:
        state = _capture_original_source(root, descriptor)
    finally:
        os.close(descriptor)
    try:
        acquired.require_current()
        entry = object.__new__(SnapshotLease)
        for name, value in (('_registry', registry), ('_acquired', acquired),
                            ('_state', state), ('_manifests', paths), ('_witnesses', {}),
                            ('_edited', None)):
            object.__setattr__(entry, name, value)
        _ENTRIES[id(entry)] = entry
        _ENTRY_RECORDS[id(entry)] = (registry, acquired, state, paths, entry._witnesses)
        _attach_entry(registry, entry)
        return entry
    except BaseException:
        state.close()
        raise


def _load_snapshot_registry(lifetime):
    """Called only by the closed SDK loader, never with source path inputs."""
    owner_type, getter = _COMPILED_PREPARATION_OWNER_TYPE, _COMPILED_PREPARATION_OWNER_GETTER
    _require(type(lifetime) is SourceSdkLifetime and owner_type is not None and
             getter is not None and _COMPILED_ACQUIRED_SNAPSHOT_TYPE is not None,
             'source_registry_compiled_preparation_unavailable')
    lifetime.require_current()
    _require(_USED_LIFETIMES.get(id(lifetime)) is not lifetime,
             'source_registry_lifetime_already_issued')
    owner = getter()
    _require(type(owner) is owner_type, 'source_registry_preparation_origin')
    owner.require_current()
    owner.require_source_sdk_lifetime(lifetime)
    _USED_LIFETIMES[id(lifetime)] = lifetime
    registry = object.__new__(SnapshotLeaseRegistry)
    values = {'_owner': owner, '_owner_type': owner_type, '_owner_getter': getter,
              '_snapshot_type': _COMPILED_ACQUIRED_SNAPSHOT_TYPE,
              '_lifetime': lifetime, '_snapshots': (), '_invocations': []}
    for name, value in values.items():
        object.__setattr__(registry, name, value)
    _REGISTRIES[id(registry)] = registry
    _REGISTRY_RECORDS[id(registry)] = (owner, owner_type, getter,
        _COMPILED_ACQUIRED_SNAPSHOT_TYPE, lifetime, registry._invocations, [], [])
    entries = []
    try:
        acquisitions = owner.acquire_source_snapshots()
        _require(type(acquisitions) is tuple and 0 < len(acquisitions) <= 256,
                 'source_registry_closed_acquisition_collection')
        for acquired in acquisitions:
            entries.append(_admit_snapshot(registry, acquired))
        for entry in entries:
            entry.require_current()
        return registry
    except BaseException:
        registry.close()
        raise
