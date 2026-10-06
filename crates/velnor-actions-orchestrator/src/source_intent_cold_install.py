"""Execute fixed fresh-install stages and observe the complete SDK genesis."""
import hashlib
import os
import stat
import tempfile
from types import MappingProxyType

import json
from source_archive_inventory_common import MAX_MANIFEST
from source_intent_cold_common import ColdSourceIntent
from source_archive_inventory_fs import root_descriptor as _root_descriptor
from source_intent_cold_process import observe_install_child as _observe_child
from source_intent_cold_recipe import _LEAVES, _compiled_recipe, _record_digest, _runtime_environment

_TOOLS = ('cargo', 'rustc', 'rustdoc')
_INSTALLATION_SEAL = object()


def _regular_hash(path, executable=True):
    descriptor = _binary_descriptor(path)
    try:
        return _descriptor_hash(descriptor, executable)
    finally:
        os.close(descriptor)


def _binary_descriptor(path):
    parent = _root_descriptor(os.path.dirname(path))
    try:
        return os.open(os.path.basename(path), os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                       dir_fd=parent)
    finally:
        os.close(parent)


def _descriptor_hash(descriptor, executable=True):
    digest, size = hashlib.sha256(), 0
    os.lseek(descriptor, 0, os.SEEK_SET)
    with os.fdopen(descriptor, 'rb', closefd=False) as stream:
        before = os.fstat(stream.fileno())
        if (not stat.S_ISREG(before.st_mode) or before.st_mode & 0o7022
                or executable and not before.st_mode & 0o111):
            raise ColdSourceIntent('cold_sdk_executable_type')
        while chunk := stream.read(1024 * 1024):
            size += len(chunk)
            if size > 1024 * 1024 * 1024:
                raise ColdSourceIntent('cold_sdk_executable_limit')
            digest.update(chunk)
        after = os.fstat(stream.fileno())
        stable = lambda item: (item.st_dev, item.st_ino, item.st_mode, item.st_nlink,
                               item.st_size, item.st_mtime_ns, item.st_ctime_ns)
        if stable(before) != stable(after):
            raise ColdSourceIntent('cold_sdk_executable_changed')
    return digest.hexdigest()


def _observe_verified(path, expected_sha256, arguments, environment, root, limit):
    descriptor = _binary_descriptor(path)
    try:
        if _descriptor_hash(descriptor) != expected_sha256:
            raise ColdSourceIntent('cold_sdk_spawn_executable_changed')
        return _observe_child([path, *arguments], environment, root, limit, 10,
                              executable_descriptor=descriptor)
    finally:
        os.close(descriptor)


def _run_stage(stage, environment):
    # Scripts come only from the exact source-owner factory, never caller files.
    with tempfile.TemporaryDirectory(prefix='velnor-cold-source-', dir=environment['RUNNER_TEMP']) as directory:
        path = directory + '/stage.sh'
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(stage['source'].encode('utf-8'))
        with open(path, 'rb') as stream:
            actual = hashlib.sha256(stream.read()).hexdigest()
        if actual != stage['source_sha256']:
            raise ColdSourceIntent('cold_sdk_stage_changed')
        environment = dict(environment, HOME=directory)
        output, status, wall = _observe_child(
            ['/bin/bash', '--noprofile', '--norc', '-p', path, *stage['arguments']],
            environment, environment['RUNNER_TEMP'], 8 * 1024 * 1024, 900)
        if status != 0:
            raise ColdSourceIntent('cold_sdk_stage_failed:' + stage['name'])
        return {'name': stage['name'], 'source_sha256': actual, 'exit_code': status,
                'wall_ns': wall, 'stdout_sha256': hashlib.sha256(output).hexdigest()}


def _require_cleared(root):
    descriptor = _root_descriptor(root)
    try:
        info = os.fstat(descriptor)
        if info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) & 0o022:
            raise ColdSourceIntent('cold_sdk_root_owner')
        if not set(os.listdir(descriptor)) <= set(_LEAVES):
            raise ColdSourceIntent('cold_sdk_unowned_root_entry')
        for name in os.listdir(descriptor):
            child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=descriptor)
            try:
                if os.listdir(child):
                    raise ColdSourceIntent('cold_sdk_root_not_empty')
            finally:
                os.close(child)
    finally:
        os.close(descriptor)


def _entries(data):
    if type(data) is not bytes or len(data) > MAX_MANIFEST:
        raise ColdSourceIntent("cold_sdk_inventory_limit")
    value = json.loads(data)
    if type(value) is not dict or value.get('schema') != 3 or type(value.get('entries')) is not list:
        raise ColdSourceIntent('cold_sdk_inventory')
    return {entry['path']: entry for entry in value['entries']}


def _tool_observations(root, recipe, environment, context):
    with tempfile.TemporaryDirectory(prefix='velnor-cold-home-', dir=environment['RUNNER_TEMP']) as home:
        return _observe_tools(root, recipe, dict(environment, HOME=home), context)


def _guarded_query(context, recipe, baseline, installed, path, digest, arguments, environment, limit):
    from source_intent_cold_manifest import source_original_inventory
    _before_query(context, recipe, baseline, installed)
    output, status, _wall = _observe_verified(path, digest, arguments, environment, context.root, limit)
    current = source_original_inventory(context).canonical_bytes
    _before_query(context, recipe, current, installed)
    if status != 0:
        raise ColdSourceIntent('cold_sdk_observation_failed')
    return output, current


def _observe_obligations(context, recipe, environment, entries, baseline):
    manager = context.root + '/cargo/bin/rustup'
    actual = {}
    for name, argument in (('components', 'component'), ('targets', 'target')):
        output, baseline = _guarded_query(context, recipe, baseline, entries, manager,
            recipe['manager_sha256'], [argument, 'list', '--installed', '--toolchain',
                                      recipe['toolchain']], environment, 16384)
        lines = output.decode('utf-8', errors='strict').splitlines()
        if len(lines) != len(set(lines)) or any(not line or line.strip() != line for line in lines):
            raise ColdSourceIntent('cold_sdk_installed_obligations_format')
        expected = recipe['expected_obligations'][name]
        if name == 'components':
            suffix = '-' + recipe['host']
            lines = [line[:-len(suffix)] if line.endswith(suffix) else line for line in lines]
        if sorted(lines) != sorted(expected):
            raise ColdSourceIntent('cold_sdk_installed_obligations')
        actual[name] = output.decode('utf-8', errors='strict')
    return actual, baseline


def _observe_tools(root, recipe, environment, context):
    from source_intent_cold_manifest import source_original_inventory
    initial = source_original_inventory(context).canonical_bytes
    entries, identities, tools, baseline = _entries(initial), {}, {}, initial
    manager = root + '/cargo/bin/rustup'
    obligations, baseline = _observe_obligations(context, recipe, environment, entries, baseline)
    for tool in recipe['expected_obligations']['commands']:
        output, baseline = _guarded_query(context, recipe, baseline, entries, manager,
            recipe['manager_sha256'], ['which', '--toolchain', recipe['toolchain'], tool],
            environment, 4096)
        relative = 'rustup-home/toolchains/' + recipe['toolchain'] + '/bin/' + tool
        path, entry = root + '/' + relative, entries.get(relative)
        if (output != (path + '\n').encode() or type(entry) is not dict
                or entry.get('kind') != 'file' or _regular_hash(path) != entry.get('sha256')):
            raise ColdSourceIntent('cold_sdk_tool_binding')
        if tool in _TOOLS:
            executable, digest, arguments = path, entry['sha256'], ['--version', '--verbose']
        else:
            executable, digest = manager, recipe['manager_sha256']
            arguments = ['run', recipe['toolchain'], tool, '--version']
        output, baseline = _guarded_query(context, recipe, baseline, entries, executable,
                                          digest, arguments, environment, 16384)
        if not output or b'\0' in output:
            raise ColdSourceIntent('cold_sdk_tool_version')
        if tool in ('rustc', 'rustdoc') and not output.startswith(
                (tool + ' ' + recipe['rust_version'] + ' ').encode()):
            raise ColdSourceIntent('cold_sdk_tool_version')
        if tool == 'cargo' and not output.startswith(b'cargo '):
            raise ColdSourceIntent('cold_sdk_tool_version')
        if tool == 'rustc' and b'\nhost: x86_64-unknown-linux-gnu\n' not in output:
            raise ColdSourceIntent('cold_sdk_compiler_host')
        identities[tool] = output.decode('utf-8', errors='strict')
        if tool in _TOOLS:
            tools[tool] = (path, entry['sha256'])
    # Authorized bootstrap queries may create Cargo state. All installed files
    # remain identical throughout; freeze the complete namespace after queries.
    final = source_original_inventory(context).canonical_bytes
    _before_query(context, recipe, final, entries)
    return identities, tools, final, obligations


def _before_query(context, recipe, baseline, installed):
    from source_intent_cold_manifest import source_original_inventory
    context.require_current()
    current = source_original_inventory(context).canonical_bytes
    if current != baseline:
        raise ColdSourceIntent('cold_sdk_bootstrap_interphase_mutation')
    entries, root = _entries(current), context.root
    proxies = ('rustup', 'cargo', 'rustc', 'rustdoc', 'cargo-clippy', 'clippy-driver', 'rustfmt', 'cargo-fmt')
    for proxy in proxies:
        relative = 'cargo/bin/' + proxy
        entry = entries.get(relative)
        if (entry != installed.get(relative) or type(entry) is not dict
                or entry.get('kind') != 'file' or entry.get('sha256') != recipe['manager_sha256']
                or _regular_hash(root + '/' + relative) != recipe['manager_sha256']):
            raise ColdSourceIntent('cold_sdk_manager_proxy_binding')
    settings = entries.get('rustup-home/settings.toml')
    if (type(settings) is not dict or settings.get('kind') != 'file'
            or settings != installed.get('rustup-home/settings.toml')
            or _regular_hash(root + '/rustup-home/settings.toml', executable=False) != settings.get('sha256')):
        raise ColdSourceIntent('cold_sdk_settings_binding')
    immutable = lambda path: (path in ('rustup-home', 'cargo/bin')
                             or path.startswith(('rustup-home/', 'cargo/bin/')))
    if ({path: entry for path, entry in entries.items() if immutable(path)}
            != {path: entry for path, entry in installed.items() if immutable(path)}):
        raise ColdSourceIntent('cold_sdk_compiler_image_changed')


class FreshColdInstallationWitness:
    """Actual source-owned waits and artifact checks, before SDK authority."""
    __slots__ = ('_recipe', '_root', '_phases', '_environment', '_foundation', '_seal')

    def __init__(self, recipe, root, phases, environment, foundation, *, _seal=None):
        if _seal is not _INSTALLATION_SEAL:
            raise ColdSourceIntent('cold_sdk_installation_witness_authority')
        from source_intent_cold_foundation import FreshPreparationFoundation
        if type(foundation) is not FreshPreparationFoundation:
            raise ColdSourceIntent('cold_sdk_foundation_issuer_authority')
        foundation.require_purpose(recipe['purpose'])
        object.__setattr__(self, '_foundation', foundation)
        object.__setattr__(self, '_recipe', MappingProxyType(recipe))
        object.__setattr__(self, '_root', root)
        object.__setattr__(self, '_phases', tuple(MappingProxyType(item) for item in phases))
        object.__setattr__(self, '_environment', MappingProxyType(dict(environment)))
        object.__setattr__(self, '_seal', _seal)

    def __setattr__(self, _name, _value):
        raise ColdSourceIntent('cold_sdk_installation_witness_immutable')

    def require_current(self):
        if getattr(self, '_seal', None) is not _INSTALLATION_SEAL:
            raise ColdSourceIntent('cold_sdk_installation_witness_authority')
        self._foundation.require_purpose(self._recipe['purpose'])
        recipe = _compiled_recipe()
        if dict(self._recipe) != recipe:
            raise ColdSourceIntent('cold_sdk_installation_source_changed')
        root, environment = _runtime_environment(recipe)
        if root != self._root or dict(self._environment) != environment:
            raise ColdSourceIntent('cold_sdk_installation_environment_changed')
        if (_regular_hash(self._root + '/mise/bin/mise') != recipe['mise_sha256']
                or _regular_hash(self._root + '/cargo/bin/rustup') != recipe['manager_sha256']):
            raise ColdSourceIntent('cold_sdk_installation_artifact_changed')

    @property
    def root(self):
        return self._root

    @property
    def installer_source_identity(self):
        return self._recipe['installer_source_identity']

    @property
    def mise_qualification_sha256(self):
        return self._recipe['mise_qualification_sha256']


def execute_cold_installation():
    """Private genesis after actual fresh stages; no SDK or JSON grant."""
    recipe = _compiled_recipe()
    from source_intent_cold_foundation import require_preparation_foundation
    foundation = require_preparation_foundation(recipe['purpose'])
    root, environment = _runtime_environment(recipe)
    foundation.require_current()
    phases = [_run_stage(recipe['stages'][0], environment)]
    _require_cleared(root)
    foundation.require_current()
    phases.append(_run_stage(recipe['stages'][1], environment))
    foundation.require_current()
    mise = root + '/mise/bin/mise'
    if _regular_hash(mise) != recipe['mise_sha256']:
        raise ColdSourceIntent('cold_sdk_acquired_mise_digest')
    foundation.require_current()
    phases.append(_run_stage(recipe['stages'][2], environment))
    foundation.require_current()
    if (_regular_hash(mise) != recipe['mise_sha256']
            or _regular_hash(root + '/cargo/bin/rustup') != recipe['manager_sha256']):
        raise ColdSourceIntent('cold_sdk_installed_manager_digest')
    witness = FreshColdInstallationWitness(recipe, root, phases, environment, foundation,
                                          _seal=_INSTALLATION_SEAL)
    witness.require_current()
    return witness


def observe_cold_installation(witness, context):
    witness.require_current()
    recipe, root = dict(witness._recipe), witness.root
    identities, tools, manifest, obligations = _tool_observations(root, recipe, dict(witness._environment), context)
    observations = {'schema': 1, 'purpose': recipe['purpose'],
                    'phases': [dict(item) for item in witness._phases],
                    'host': recipe['host'], 'installer_source_identity': recipe['installer_source_identity'],
                    'mise_qualification_sha256': recipe['mise_qualification_sha256'],
                    'foundation_qualification_sha256': context._profile['qualification_sha256'],
                    'installed_manifest_sha256': hashlib.sha256(manifest).hexdigest(),
                    'tool_identities': identities, 'installed_obligations': obligations,
                    'recipe_sha256': _record_digest(recipe)}
    return manifest, tools, observations
