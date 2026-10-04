"""Observe one private native candidate. No receipt or SDK authority escapes."""
import hashlib
import json
import os
import tempfile

from source_intent_cold_common import ColdSourceIntent
from source_intent_cold_install import _binary_descriptor, _descriptor_hash
from source_root_rust_candidate_install import execute_root_rust_candidate, _observe_candidate_verified
from source_root_rust_candidate_context import _from_completed_candidate
from source_root_rust_candidate_manifest import inventory_candidate_root

_COMMANDS = ('rustc', 'cargo', 'rustdoc', 'cargo-clippy', 'clippy-driver', 'rustfmt', 'cargo-fmt')


def _validate_payload(context, recipe, manifest):
    entries = {entry['path']: entry for entry in json.loads(manifest)['entries']}
    prefix = 'rustup-home/toolchains/' + recipe['toolchain'] + '/'
    payload = recipe['native_source_authority']['payload']
    for expected in payload:
        path = prefix + expected['path']
        entry = entries.get(path)
        if (type(entry) is not dict or entry.get('kind') != 'file'
                or entry.get('sha256') != expected['sha256'] or entry.get('mode') != expected['mode']):
            raise ColdSourceIntent('root_candidate_payload_changed')
        descriptor = _binary_descriptor(context.root + '/' + path)
        try:
            if (os.fstat(descriptor).st_size != expected['size']
                    or _descriptor_hash(descriptor, executable=bool(expected['mode'] & 0o111))
                    != expected['sha256']):
                raise ColdSourceIntent('root_candidate_payload_changed')
        finally:
            os.close(descriptor)
    expected_bins = {prefix + item['path'] for item in payload if item['path'].startswith('bin/')}
    actual_bins = {path for path, entry in entries.items()
                   if path.startswith(prefix + 'bin/') and entry.get('kind') != 'directory'}
    if actual_bins != expected_bins:
        raise ColdSourceIntent('root_candidate_extra_compiler')
    return entries


def _query(context, recipe, baseline, entries, path, digest, arguments, environment, limit):
    context.require_current()
    if inventory_candidate_root(context) != baseline:
        raise ColdSourceIntent('root_candidate_pre_spawn_mutation')
    _validate_payload(context, recipe, baseline)
    output, status, _wall = _observe_candidate_verified(path, digest, arguments, environment, context.root, limit)
    context.require_current()
    if inventory_candidate_root(context) != baseline:
        raise ColdSourceIntent('root_candidate_post_spawn_mutation')
    if status != 0:
        raise ColdSourceIntent('root_candidate_observation_failed')
    return output


def _observe_candidate(context, witness, environment):
    recipe = dict(witness._recipe)
    baseline = inventory_candidate_root(context)
    entries = _validate_payload(context, recipe, baseline)
    manager = context.root + '/cargo-home/bin/rustup'
    identities = {}
    for command in _COMMANDS:
        path = context.root + '/rustup-home/toolchains/' + recipe['toolchain'] + '/bin/' + command
        output = _query(context, recipe, baseline, entries, manager, recipe['manager_sha256'],
                        ['which', '--toolchain', recipe['toolchain'], command], environment, 4096)
        if output != (path + '\n').encode():
            raise ColdSourceIntent('root_candidate_selected_tool')
        relative = path[len(context.root) + 1:]
        if command in ('cargo', 'rustc', 'rustdoc'):
            program, digest, arguments = path, entries[relative]['sha256'], ['--version', '--verbose']
        else:
            program, digest = manager, recipe['manager_sha256']
            arguments = ['run', recipe['toolchain'], command, '--version']
        output = _query(context, recipe, baseline, entries, program, digest, arguments, environment, 16384)
        if not output or b'\0' in output:
            raise ColdSourceIntent('root_candidate_tool_version')
        if command in ('rustc', 'rustdoc') and not output.startswith(
                (command + ' ' + recipe['rust_version'] + ' ').encode()):
            raise ColdSourceIntent('root_candidate_tool_version')
        if command == 'cargo' and not output.startswith(b'cargo '):
            raise ColdSourceIntent('root_candidate_tool_version')
        if command == 'rustc' and b'\nhost: x86_64-unknown-linux-gnu\n' not in output:
            raise ColdSourceIntent('root_candidate_compiler_host')
        identities[command] = output.decode('utf-8', errors='strict')
    obligations = _obligations(context, recipe, baseline, entries, manager, environment)
    return {'schema': 1, 'purpose': 'root-linux-compiler-candidate-observation',
            'host': recipe['host'], 'rust_version': recipe['rust_version'],
            'installer_source_identity': recipe['installer_source_identity'],
            'native_source_identity': recipe['native_source_authority']['qualification_sha256'],
            'phases': [dict(item) for item in witness._phases], 'tool_identities': identities,
            'installed_obligations': obligations,
            'original_manifest_sha256': hashlib.sha256(baseline).hexdigest()}


def _obligations(context, recipe, baseline, entries, manager, environment):
    actual = {}
    for name, argument, expected in (
            ('components', 'component', ['cargo', 'rustc', 'rust-std', 'clippy', 'rustfmt']),
            ('targets', 'target', [recipe['host']])):
        output = _query(context, recipe, baseline, entries, manager, recipe['manager_sha256'],
                        [argument, 'list', '--installed', '--toolchain', recipe['toolchain']],
                        environment, 16384)
        lines = output.decode('utf-8', errors='strict').splitlines()
        if len(lines) != len(set(lines)) or any(not line or line.strip() != line for line in lines):
            raise ColdSourceIntent('root_candidate_obligations_format')
        if name == 'components':
            suffix = '-' + recipe['host']
            lines = [line[:-len(suffix)] if line.endswith(suffix) else line for line in lines]
        if sorted(lines) != sorted(expected):
            raise ColdSourceIntent('root_candidate_obligations')
        actual[name] = output.decode('utf-8', errors='strict')
    return actual


def observe_root_rust_candidate():
    """Zero arguments. All source, namespace and launch authority are owner bound."""
    witness = execute_root_rust_candidate()
    context = _from_completed_candidate(witness)
    try:
        with tempfile.TemporaryDirectory(prefix='velnor-root-rust-home-',
                                         dir=witness._environment['RUNNER_TEMP']) as home:
            return _observe_candidate(context, witness, dict(witness._environment, HOME=home))
    finally:
        context.close()
