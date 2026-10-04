"""Closed native observation validation; JSON/filesystem data confer no authority."""
import base64
import hashlib
import json
import os
from types import MappingProxyType


class NativeSourceSemanticsUnavailable(RuntimeError):
    pass


def _require(condition, reason):
    if not condition:
        raise NativeSourceSemanticsUnavailable(reason)


def _canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'),
                      ensure_ascii=True, allow_nan=False,
                      default=_plain_mapping).encode('ascii')


def _plain_mapping(value):
    _require(type(value) is MappingProxyType, 'native_semantics_non_json_data')
    return dict(value)


def _fields(value, fields, reason):
    _require(type(value) is dict and set(value) == set(fields), reason)


def _validate_context(context, locked):
    _fields(context, ('requested_manifest', 'workspace_root', 'member_manifests',
                     'governingLockContext', 'governing_lockfile_sha256'), 'native_semantics_context')
    _require(type(context['requested_manifest']) is str and
             type(context['workspace_root']) is str and
             type(context['member_manifests']) is list and
             all(type(item) is str for item in context['member_manifests']),
             'native_semantics_context_paths')
    governing = context['governingLockContext']
    _fields(governing, ('format', 'workspaceManifest', 'governingLockfile',
                       'lockfileBytesBase64'), 'native_semantics_governing_fields')
    _require(type(governing['format']) is int and governing['format'] == 1,
             'native_semantics_governing_format')
    raw, digest = governing['lockfileBytesBase64'], context['governing_lockfile_sha256']
    if raw is None:
        _require(not locked and digest is None, 'native_semantics_lock_required')
    else:
        _require(type(raw) is str and type(digest) is str, 'native_semantics_lock_shape')
        try:
            data = base64.b64decode(raw, validate=True)
        except ValueError as error:
            raise NativeSourceSemanticsUnavailable('native_semantics_lock_encoding') from error
        _require(hashlib.sha256(data).hexdigest() == digest, 'native_semantics_lock_digest')


def _validate_result(result, operation, before, compiler_paths):
    _fields(result, ('format', 'operation', 'metadata', 'native_context',
                     'compiler_context', 'observations'), 'native_semantics_result_fields')
    _require(type(result['format']) is int and result['format'] == 1 and
             result['operation'] == operation and type(result['metadata']) is dict and
             type(result['observations']) is list, 'native_semantics_result_binding')
    for observation in result['observations']:
        _fields(observation, ('source_id', 'kind', 'path', 'sha256'),
                'native_semantics_registry_observation')
        _require(all(type(observation[key]) is str for key in ('source_id', 'kind', 'path')),
                 'native_semantics_registry_observation_strings')
        digest = observation['sha256']
        _require(digest is None or type(digest) is str and len(digest) == 64 and
                 all(char in '0123456789abcdef' for char in digest),
                 'native_semantics_registry_observation_digest')
    contexts = result['native_context']
    _fields(contexts, ('before', 'after'), 'native_semantics_context_pair')
    for context in contexts.values():
        _validate_context(context, operation != 'manifest_read')
    _require(_canonical(contexts['before']) == _canonical(before),
             'native_semantics_before_context_changed')
    if operation in ('manifest_read', 'full_locked_metadata'):
        _require(_canonical(contexts['after']) == _canonical(contexts['before']),
                 'native_semantics_read_context_mutated')
    else:
        raise NativeSourceSemanticsUnavailable(
            'native_semantics_expected_governing_transition_and_lifetime_issuer_absent')
    compiler = result['compiler_context']
    if compiler == {'kind': 'not_invoked'}:
        _require(operation == 'manifest_read', 'native_semantics_compiler_required')
        return
    _fields(compiler, ('kind', 'rustc_path', 'stock_cargo_path', 'verbose_version',
                      'version', 'host', 'commit_hash', 'wrapper', 'workspace_wrapper'),
            'native_semantics_compiler_context')
    _require(compiler['kind'] == 'native_rustc' and compiler['wrapper'] is None and
             compiler['workspace_wrapper'] is None and compiler['version'] == '1.98.1' and
             compiler['host'] == 'x86_64-unknown-linux-gnu' and
             compiler['rustc_path'] == compiler_paths['rustc'] and
             compiler['stock_cargo_path'] == compiler_paths['cargo'] and
             type(compiler['verbose_version']) is str and type(compiler['commit_hash']) is str,
             'native_semantics_compiler_binding')


def _validate_comparison(result, request, source_root, source_manifest):
    # Full neutral inventory seals lexical link routes as well as target bytes.
    from source_archive_inventory_fs import root_descriptor
    from source_archive_inventory_original import _original_inventory
    from source_archive_inventory_common import InventoryError
    try:
        descriptor = root_descriptor(source_root)
        try:
            current = _original_inventory(source_root, tuple(sorted(os.listdir(descriptor))),
                                          descriptor).canonical_bytes
        finally:
            os.close(descriptor)
    except (InventoryError, OSError) as error:
        raise NativeSourceSemanticsUnavailable('native_semantics_comparison_inventory') from error
    _require(current == source_manifest, 'native_semantics_comparison_source_layout_changed')
    _fields(result, ('format', 'package_name', 'package_version', 'files'),
            'native_semantics_comparison_result')
    _require(type(result['format']) is int and result['format'] == 1 and
             result['package_name'] == request['package_name'] and
             type(result['package_version']) is str and type(result['files']) is list,
             'native_semantics_comparison_binding')
    entries = {entry['path']: entry for entry in json.loads(source_manifest)['entries']}
    roles = {'source', 'original_manifest', 'normalized_manifest', 'lockfile', 'vcs_info'}
    for item in result['files']:
        _fields(item, ('path', 'role', 'source_path'), 'native_semantics_comparison_file')
        _require(type(item['path']) is str and type(item['role']) is str and item['role'] in roles,
                 'native_semantics_comparison_file_shape')
        source = item['source_path']
        if source is None:
            _require(item['role'] not in ('source', 'original_manifest'),
                     'native_semantics_comparison_source_missing')
            continue
        _require(type(source) is str and '\0' not in source,
                 'native_semantics_comparison_source_path')
        joined = os.path.join(os.path.dirname(request['manifest_path']), source)
        path = _sealed_source_route(joined, source_root, entries)
        _require(os.path.realpath(joined) == path,
                 'native_semantics_comparison_source_route_changed')
        _require(os.path.commonpath((source_root, path)) == source_root,
                 'native_semantics_comparison_source_escape')
        entry = entries.get(os.path.relpath(path, source_root))
        _require(type(entry) is dict and entry.get('kind') == 'file',
                 'native_semantics_comparison_source_binding')



def _sealed_source_route(joined, source_root, entries):
    from source_archive_inventory_common import direct_target, InventoryError
    _require(joined.startswith(source_root + '/'),
             'native_semantics_comparison_source_route_escape')
    pending, parts = joined[len(source_root) + 1:].split('/'), []
    while pending:
        component = pending.pop(0)
        if component in ('', '.'):
            continue
        if component == '..':
            _require(bool(parts), 'native_semantics_comparison_source_route_escape')
            parts.pop()
            continue
        relative = '/'.join(parts + [component])
        entry = entries.get(relative)
        _require(type(entry) is dict, 'native_semantics_comparison_source_route_unsealed')
        if entry.get('kind') == 'symlink':
            try:
                relative = direct_target(relative, entry['target'], entries,
                                         frozenset(), source_root)
            except InventoryError as error:
                raise NativeSourceSemanticsUnavailable(
                    'native_semantics_comparison_source_route_escape') from error
            parts = relative.split('/')
            entry = entries[relative]
        else:
            parts.append(component)
        _require(entry.get('kind') == ('directory' if pending else 'file'),
                 'native_semantics_comparison_source_route_kind')
    _require(entries.get('/'.join(parts), {}).get('kind') == 'file',
             'native_semantics_comparison_source_binding')
    return source_root + '/' + '/'.join(parts)
