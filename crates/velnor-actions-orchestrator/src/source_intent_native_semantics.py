"""Closed owned Cargo source semantics; request/response DTOs confer no authority.

Adopted source, installation and semantic Foundation lifetime issuance is absent.
The zero argument loader rejects that absence before any filesystem/process work.
Documentation execution is outside this SDK's purpose.
"""
import json
import os
import stat
from types import MappingProxyType
# The immutable source composer binds the validation helper into this namespace
# before this SDK. It also binds the actual cold SDK classes; no path importer.

_COMPILED_NATIVE_SOURCE_SEMANTICS_ROLE = None
_COMPILED_NATIVE_SEMANTIC_LIFETIME = None
_ROLE_SEAL, _PAIR_SEAL, _OPERAND_SEAL, _SDK_SEAL, _LIFETIME_SEAL = (
    object(), object(), object(), object(), object())
_OPERATIONS = frozenset(('manifest_read', 'full_locked_metadata',
                         'workspace_version_lock_mutation'))


class _ColdCompilerPair:
    __slots__ = ('_sdk', '_tools', '_seal')

    def __init__(self, sdk, *, _seal=None):
        try:
            sdk_type, tool_type = ColdSourceIntentSdk, ColdSourceIntentInstalledTool
        except NameError as error:
            raise NativeSourceSemanticsUnavailable('native_semantics_fixed_cold_owner_absent') from error
        _require(_seal is _PAIR_SEAL and type(sdk) is sdk_type,
                 'native_semantics_cold_compiler_origin')
        sdk.require_policy('1.98.1', 'x86_64-unknown-linux-gnu')
        tools = {name: sdk.installed_tool(name) for name in ('cargo', 'rustc', 'rustdoc')}
        _require(all(type(tool) is tool_type for tool in tools.values()),
                 'native_semantics_cold_tool_origin')
        parents = {os.path.dirname(tool.path) for tool in tools.values()}
        _require(len(parents) == 1, 'native_semantics_cold_tool_root')
        object.__setattr__(self, '_sdk', sdk)
        object.__setattr__(self, '_tools', MappingProxyType(tools))
        object.__setattr__(self, '_seal', _seal)
        self.require_current()

    def __setattr__(self, _name, _value):
        raise NativeSourceSemanticsUnavailable('native_semantics_compiler_immutable')

    def require_current(self):
        _require(getattr(self, '_seal', None) is _PAIR_SEAL,
                 'native_semantics_compiler_authority')
        self._sdk.require_policy('1.98.1', 'x86_64-unknown-linux-gnu')
        for tool in self._tools.values():
            tool.require_current()


class _NativeSemanticLifetime:
    """Private original namespace/lifetime issuer; observations cannot issue it."""
    __slots__ = ('_seal', '_descriptor', '_identity', '_receipt')

    def __init__(self):
        raise NativeSourceSemanticsUnavailable('native_semantics_lifetime_issuer_absent')

    def __setattr__(self, _name, _value):
        raise NativeSourceSemanticsUnavailable('native_semantics_lifetime_immutable')

    def require_current(self):
        _require(self is _COMPILED_NATIVE_SEMANTIC_LIFETIME and
                 getattr(self, '_seal', None) is _LIFETIME_SEAL and
                 self._receipt is not None and self._descriptor is not None,
                 'native_semantics_namespace_lifetime_unavailable')
        info = os.fstat(self._descriptor)
        _require((info.st_dev, info.st_ino) == self._identity,
                 'native_semantics_namespace_identity_changed')
        raise NativeSourceSemanticsUnavailable(
            'native_semantics_fixed_sourcequalified_namespace_launcher_issuer_absent')


class _NativeSemanticsSourceRole:
    """Opaque compiled issuer, with no path/dictionary/receipt constructor."""
    __slots__ = ('_seal', '_pair', '_operand', '_lifetime', '_executable',
                 '_executable_sha256', '_installed_root', '_manifest', '_receipts', '_source_revision')

    def __init__(self):
        raise NativeSourceSemanticsUnavailable('native_semantics_owned_source_issuer_absent')

    def __setattr__(self, _name, _value):
        raise NativeSourceSemanticsUnavailable('native_semantics_source_role_immutable')

    def require_current(self):
        _require(self is _COMPILED_NATIVE_SOURCE_SEMANTICS_ROLE and
                 getattr(self, '_seal', None) is _ROLE_SEAL,
                 'native_semantics_owned_source_authority')
        _require(type(self._pair) is _ColdCompilerPair and
                 type(self._operand) is _NativeSourceOperand and
                 type(self._lifetime) is _NativeSemanticLifetime and
                 self._receipts is not None,
                 'native_semantics_actual_issuer_receipts_absent')
        self._lifetime.require_current()
        self._pair.require_current()
        _require(_inventory(self._installed_root)[0] == self._manifest,
                 'native_semantics_owned_installed_closure_changed')
        _require(_file_sha256(self._executable) == self._executable_sha256,
                 'native_semantics_owned_executable_changed')
        self._lifetime.require_current()


class _NativeSourceOperand:
    """Private source-issued original/governing context; DTOs grant nothing."""
    __slots__ = ('_seal', '_role', '_request_bytes', '_context_bytes', '_root',
                 '_root_identity', '_source_manifest', '_work', '_work_identity', '_comparison_request_bytes')

    def __init__(self):
        raise NativeSourceSemanticsUnavailable('native_semantics_preparation_operand_issuer_absent')

    def __setattr__(self, _name, _value):
        raise NativeSourceSemanticsUnavailable('native_semantics_operand_immutable')

    def require_current(self):
        _require(getattr(self, '_seal', None) is _OPERAND_SEAL and
                 type(self._role) is _NativeSemanticsSourceRole and
                 self._role._operand is self,
                 'native_semantics_operand_authority')
        self._role.require_current()
        _require(_inventory(self._root)[1] == self._root_identity and
                 _work_identity(self._work) == self._work_identity,
                 'native_semantics_operand_root_changed')
        _require(_inventory(self._root)[0] == self._source_manifest,
                 'native_semantics_original_source_changed')
        self._role._lifetime.require_current()


class NativeSourceSemanticsSdk:
    __slots__ = ('_role', '_seal')

    def __init__(self, role, *, _seal=None):
        _require(_seal is _SDK_SEAL and type(role) is _NativeSemanticsSourceRole,
                 'native_semantics_sdk_origin')
        role.require_current()
        object.__setattr__(self, '_role', role)
        object.__setattr__(self, '_seal', _seal)

    def __setattr__(self, _name, _value):
        raise NativeSourceSemanticsUnavailable('native_semantics_sdk_immutable')

    def require_current(self):
        _require(getattr(self, '_seal', None) is _SDK_SEAL, 'native_semantics_sdk_authority')
        self._role.require_current()

    def source_operand(self):
        self.require_current()
        operand = self._role._operand
        operand.require_current()
        return operand

    def require_source_operand(self, operand):
        self.require_current()
        _require(type(operand) is _NativeSourceOperand and operand is self._role._operand,
                 'native_semantics_foreign_operand')
        operand.require_current()

    def cargo_identity(self):
        self.require_current()
        self._role._pair.require_current()
        # No banner constant or source hash may impersonate the fixed live stock
        # Cargo query. Its sourcequalified namespace query issuer is not present.
        raise NativeSourceSemanticsUnavailable(
            'native_semantics_stock_cargo_identity_namespace_query_issuer_absent')

    def manifest_read(self, operand):
        return self._operation(operand, 'manifest_read', None)

    def full_locked_metadata(self, operand, governingLockContext):
        return self._operation(operand, 'full_locked_metadata', governingLockContext)

    def workspace_version_lock_mutation(self, operand, governingLockContext):
        return self._operation(operand, 'workspace_version_lock_mutation', governingLockContext)

    def _operation(self, operand, operation, governing):
        self.require_source_operand(operand)
        _require(operation != 'workspace_version_lock_mutation',
                 'native_semantics_expected_governing_transition_and_lifetime_issuer_absent')
        held_context = json.loads(operand._context_bytes)
        _validate_context(held_context, operation != 'manifest_read')
        if operation != 'manifest_read':
            _require(governing is not None, 'native_semantics_governing_context_required')
            _require(_canonical(governing) == _canonical(held_context['governingLockContext']),
                     'native_semantics_governing_context_substituted')
        request = json.loads(operand._request_bytes)
        _fields(request, ('format', 'source_root', 'manifest_path', 'work_root',
                         'toolchain_root', 'registries', 'operation'), 'native_semantics_request')
        _require(type(request['format']) is int and request['format'] == 1 and
                 request['source_root'] == operand._root and request['work_root'] == operand._work,
                 'native_semantics_request_binding')
        request['operation'] = {'kind': operation}
        try:
            result = _execute(self._role, operand, request)
            _validate_result(result, operation, held_context,
                             {name: tool.path for name, tool in self._role._pair._tools.items()})
            return result
        finally:
            try:
                operand.require_current()
            finally:
                self.require_current()

    def comparison_read(self, operand):
        self.require_source_operand(operand)
        request = json.loads(operand._comparison_request_bytes)
        _fields(request, ('format', 'manifest_path', 'cargo_home', 'package_name'),
                'native_semantics_comparison_request')
        held = json.loads(operand._request_bytes)
        _require(type(request['format']) is int and request['format'] == 1 and
                 request['manifest_path'] == held['manifest_path'],
                 'native_semantics_comparison_manifest_binding')
        try:
            operand.require_current()
            self._role._lifetime.require_current()
            result = _execute_comparison(self._role, operand, request)
            _validate_comparison(result, request, operand._root, operand._source_manifest)
            return result
        finally:
            try:
                operand.require_current()
            finally:
                self.require_current()


def _work_identity(path):
    from source_archive_inventory_fs import root_descriptor
    descriptor = root_descriptor(path)
    try:
        info = os.fstat(descriptor)
        return info.st_dev, info.st_ino
    finally:
        os.close(descriptor)


def _inventory(path):
    from source_archive_inventory_fs import root_descriptor
    from source_archive_inventory_original import _original_inventory
    descriptor = root_descriptor(path)
    try:
        info = os.fstat(descriptor)
        _require(info.st_uid == os.geteuid() and not info.st_mode & 0o022,
                 'native_semantics_root_ownership')
        manifest = _original_inventory(path, tuple(sorted(os.listdir(descriptor))),
                                       descriptor).canonical_bytes
        return manifest, (info.st_dev, info.st_ino)
    finally:
        os.close(descriptor)


def _file_sha256(path):
    from source_archive_inventory_fs import root_descriptor, read_descriptor
    from source_archive_inventory_common import _ORIGINAL_OBSERVATION
    parent = root_descriptor(os.path.dirname(path))
    descriptor = None
    try:
        descriptor = os.open(os.path.basename(path), os.O_RDONLY | os.O_NOFOLLOW |
                             os.O_NONBLOCK, dir_fd=parent)
        info = os.fstat(descriptor)
        _require(stat.S_ISREG(info.st_mode) and not info.st_mode & 0o7022,
                 'native_semantics_executable_file')
        return read_descriptor(descriptor, info, admitted_links=True,
                               observation=_ORIGINAL_OBSERVATION)[0]
    finally:
        if descriptor is not None:
            os.close(descriptor)
        os.close(parent)


def _execute(role, operand, request):
    _require(type(role) is _NativeSemanticsSourceRole and
             type(operand) is _NativeSourceOperand and operand is role._operand,
             'native_semantics_native_operation_origin')
    _fields(request['operation'], ('kind',), 'native_semantics_operation_fields')
    _require(request['operation']['kind'] in _OPERATIONS,
             'native_semantics_operation_kind')
    operand.require_current()
    role._lifetime.require_current()
    # Endpoint source and DTO are frozen, but no authenticated fixed namespace
    # program exists. Never substitute a host subprocess or descriptor observation.
    raise NativeSourceSemanticsUnavailable(
        'native_semantics_source_semantics_endpoint_and_fixed_namespace_program_absent')


def _execute_comparison(role, operand, request):
    _require(type(role) is _NativeSemanticsSourceRole and operand is role._operand,
             'native_semantics_comparison_operation_origin')
    operand.require_current()
    role._lifetime.require_current()
    raise NativeSourceSemanticsUnavailable(
        'native_semantics_list_source_files_fixed_namespace_launcher_issuer_absent')


def load_native_source_semantics_sdk():
    """Zero inputs; source/installed artifact/lifetime/operand issuance is required."""
    role = _COMPILED_NATIVE_SOURCE_SEMANTICS_ROLE
    _require(type(role) is _NativeSemanticsSourceRole,
             'native_semantics_unavailable: adopted-owned-source/build/full-installed-closure/'
             'semantic-Foundation-lifetime/preparation-operand issuer required')
    role.require_current()
    return NativeSourceSemanticsSdk(role, _seal=_SDK_SEAL)
