"""Compiler-owned fresh SDK recipe; no cache-policy or caller-data loader."""
import hashlib
import json
import os
import platform

from source_intent_cold_common import ColdSourceIntent
from source_intent_cold_compiler import require_compiler_projection

_COMPILED_COLD_SDK_RECIPE = None
_COMPILED_COLD_SDK_QUALIFICATION = None
_LEAVES = ('mise', 'cargo', 'rustup-home', 'rustup-bootstrap', 'mise-config',
           'mise-system-config', 'trusted-bin')
_MANAGER_SHA256 = 'dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71'
_EXPECTED_OBLIGATIONS = {
    'profile': 'minimal',
    'components': ['cargo', 'rustc', 'rust-std', 'clippy', 'rustfmt'],
    'targets': ['x86_64-unknown-linux-gnu'],
    'commands': ['rustc', 'cargo', 'rustdoc', 'cargo-clippy', 'clippy-driver',
                 'rustfmt', 'cargo-fmt'],
}
_TOOL_IDENTITY_NAMES = ('rustc', 'cargo', 'rustdoc', 'cargo-clippy', 'clippy-driver',
                        'rustfmt', 'cargo-fmt')
_INSTALLED_OBLIGATION_NAMES = ('components', 'targets')


def _digest(value, length=64):
    return isinstance(value, str) and len(value) == length and all(c in '0123456789abcdef' for c in value)


def _record_digest(record):
    data = json.dumps(record, sort_keys=True, separators=(',', ':'), ensure_ascii=True,
                      allow_nan=False).encode('ascii')
    return hashlib.sha256(data).hexdigest()


def _compiled_recipe():
    record = _COMPILED_COLD_SDK_RECIPE
    fields = {'schema', 'purpose', 'role', 'host', 'rust_version', 'toolchain', 'namespace',
              'leaves', 'stages', 'environment', 'mise_sha256', 'manager_sha256',
              'installer_source_identity', 'mise_qualification_sha256', 'compiler_source_authority',
              'expected_obligations'}
    if (type(record) is not dict or set(record) != fields or type(record['schema']) is not int
            or record['schema'] != 1 or record['role'] != 'root-linux'
            or record['host'] != 'x86_64-unknown-linux-gnu' or record['rust_version'] != '1.98.1'
            or record['toolchain'] != '1.98.1-x86_64-unknown-linux-gnu'
            or record['namespace'] != 'velnor-control/source-intent'
            or tuple(record['leaves']) != _LEAVES or record['manager_sha256'] != _MANAGER_SHA256
            or record['expected_obligations'] != _EXPECTED_OBLIGATIONS):
        raise ColdSourceIntent('cold_sdk_compiled_recipe_unavailable')
    if (record['purpose'] != 'source-intent-cold-sdk' or not _digest(record['mise_sha256'])
            or not _digest(record['installer_source_identity'])
            or not _digest(record['mise_qualification_sha256'])):
        raise ColdSourceIntent('cold_sdk_source_purpose')
    require_compiler_projection(record['compiler_source_authority'])
    stages = record['stages']
    if (type(stages) is not list or any(type(stage) is not dict for stage in stages)
            or [stage.get('name') for stage in stages] != ['clear', 'acquire', 'install']):
        raise ColdSourceIntent('cold_sdk_stage_order')
    for stage in stages:
        if (type(stage) is not dict or set(stage) != {'name', 'source', 'source_sha256', 'arguments'}
                or not isinstance(stage['source'], str) or not 1 <= len(stage['source'].encode()) <= 1024 * 1024
                or not _digest(stage['source_sha256'])
                or hashlib.sha256(stage['source'].encode()).hexdigest() != stage['source_sha256']
                or type(stage['arguments']) is not list or len(stage['arguments']) > 256
                or any(not isinstance(item, str) or '\0' in item or len(item) > 8192
                       for item in stage['arguments'])):
            raise ColdSourceIntent('cold_sdk_stage_source')
    return json.loads(json.dumps(record))


def _runtime_environment(recipe):
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        raise ColdSourceIntent('cold_sdk_actual_host')
    temp = os.environ.get('RUNNER_TEMP', '')
    if (not temp.startswith('/') or os.path.realpath(temp) != temp or not os.path.isdir(temp)
            or any(part in ('', '.', '..') for part in temp.split('/')[1:])):
        raise ColdSourceIntent('cold_sdk_runner_temp')
    root = temp + '/' + recipe['namespace']
    if os.environ.get('VELNOR_SOURCE_INTENT_COLD_ROOT') != root:
        raise ColdSourceIntent('cold_sdk_namespace_binding')
    compiled = recipe['environment']
    expected = {'RUNNER_TEMP': temp, 'VELNOR_SOURCE_INTENT_COLD_ROOT': root,
                'MISE_DATA_DIR': root + '/mise', 'CARGO_HOME': root + '/cargo',
                'RUSTUP_HOME': root + '/rustup-home', 'MISE_CONFIG_DIR': root + '/mise-config',
                'MISE_SYSTEM_CONFIG_DIR': root + '/mise-system-config',
                'MISE_CARGO_HOME': root + '/cargo', 'MISE_RUSTUP_HOME': root + '/rustup-home',
                'RUSTUP_TOOLCHAIN': recipe['toolchain'], 'RUSTUP_AUTO_INSTALL': '0',
                'MISE_NO_CONFIG': '1', 'MISE_NO_ENV': '1', 'MISE_NO_HOOKS': '1',
                'MISE_LOCKFILE': '0', 'MISE_AUTO_INSTALL': 'false', 'MISE_EXEC_AUTO_INSTALL': 'false'}
    if (type(compiled) is not dict or set(compiled) != set(expected)
            or any(not isinstance(value, str) or '\0' in value for value in compiled.values())):
        raise ColdSourceIntent('cold_sdk_environment')
    resolved = {key: value.replace('${{ runner.temp }}', temp) for key, value in compiled.items()}
    if resolved != expected:
        raise ColdSourceIntent('cold_sdk_environment_roots')
    environment = dict(expected, PATH='/usr/bin:/bin:/usr/sbin:/sbin', LANG='C.UTF-8',
                       LC_ALL='C.UTF-8', TZ='UTC')
    return root, environment


def _require_qualification(recipe, observations):
    # Native component/transform qualification is a separate source-owned
    # prerequisite. Its supplier is not published; never promote a current-run
    # OriginalFS observation or matching version string into that authority.
    native = recipe.get('compiler_source_authority')
    if type(native) is not dict:
        raise ColdSourceIntent('cold_sdk_native_installation_authority_unavailable')
    require_compiler_projection(native)
    if (recipe.get('expected_obligations') != _EXPECTED_OBLIGATIONS
            or type(observations) is not dict):
        raise ColdSourceIntent('cold_sdk_expected_obligations')
    tool_identities = observations.get('tool_identities')
    if (type(tool_identities) is not dict
            or set(tool_identities) != set(_TOOL_IDENTITY_NAMES)
            or any(type(tool_identities[name]) is not str or not tool_identities[name]
                   or '\0' in tool_identities[name] or len(tool_identities[name]) > 16384
                   for name in _TOOL_IDENTITY_NAMES)):
        raise ColdSourceIntent('cold_sdk_tool_identities')
    installed_obligations = observations.get('installed_obligations')
    if (type(installed_obligations) is not dict
            or set(installed_obligations) != set(_INSTALLED_OBLIGATION_NAMES)
            or any(type(installed_obligations[name]) is not str
                   or not installed_obligations[name]
                   or '\0' in installed_obligations[name]
                   or len(installed_obligations[name]) > 1024 * 1024
                   for name in _INSTALLED_OBLIGATION_NAMES)):
        raise ColdSourceIntent('cold_sdk_installed_obligations')
    qualification = _COMPILED_COLD_SDK_QUALIFICATION
    expected = {'schema': 1, 'purpose': 'source-intent-cold-sdk',
                'recipe_sha256': _record_digest(recipe), 'host': recipe['host'],
                'installer_source_identity': recipe['installer_source_identity'],
                'mise_qualification_sha256': recipe['mise_qualification_sha256'],
                'compiler_source_authority': native,
                'foundation_qualification_sha256': observations['foundation_qualification_sha256'],
                'tool_identities': tool_identities,
                'installed_obligations': installed_obligations,
                'expected_obligations': _EXPECTED_OBLIGATIONS}
    if type(qualification) is not dict or qualification != expected:
        raise ColdSourceIntent('cold_sdk_genesis_qualification_unavailable')
