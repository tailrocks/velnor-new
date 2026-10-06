"""Closed RootLinux Rust candidate recipe; observations never grant a compiler."""
import hashlib
import json
import os
import platform
import stat

from source_intent_cold_common import ColdSourceIntent
from source_intent_cold_compiler import _manifest_identity
from source_intent_cold_foundation import FreshPreparationFoundation


_COMPILED_ROOT_RUST_CANDIDATE_RECIPE = None

_VERSION = '1.98.1'
_HOST = 'x86_64-unknown-linux-gnu'
_TOOLCHAIN = _VERSION + '-' + _HOST
_NAMESPACE = 'velnor-control/root-rust-candidate'
_ROOT_ENV = 'VELNOR_ROOT_RUST_CANDIDATE_ROOT'
_MANAGER_VERSION = '1.29.1'
_MANAGER_SHA256 = 'dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71'
_PAYLOAD_FILE_SHA256 = '55e828f3ae021b440f1fac6df570295a40f5411b6a527ffa771cdaebb71ca1f4'
_PAYLOAD_CANONICAL_SHA256 = 'bf5bbcccecd9cbf5aee18f46e53531c456aef1206400d2091e072a5107d49efd'
_MANIFEST_IDENTITY_SHA256 = 'a847265723f691be095172b116672294ab5b3e6ab793b5bfe8ea9a2185af4d6f'
_NATIVE_QUALIFICATION_SHA256 = '21f646abc53c411546f7eb7699e9f71da719c82e69380b29a13851bc631617c1'
_TRANSFORM_ABI = 'rustup-native-file-components-source-v1'
_LEAVES = ('cargo-home', 'rustup-home', 'rustup-bootstrap', 'native-dist', 'manager-bin')
_EXECUTABLE_ROSTER = (
    'bash', '/usr/bin/python3', 'curl', '/usr/bin/sha256sum', 'chmod', 'rm', 'mkdir', 'ln',
    '${VELNOR_ROOT_RUST_CANDIDATE_ROOT}/rustup-bootstrap/rustup-init',
    '${CARGO_HOME}/bin/rustup',
)
_EXECUTABLE_SEARCH_PATHS = ('/usr/bin', '/bin', '/usr/sbin', '/sbin')
_RECIPE_FIELDS = {
    'schema', 'purpose', 'role', 'host', 'rust_version', 'toolchain', 'namespace',
    'leaves', 'stages', 'environment', 'manager_sha256', 'manager_version',
    'native_source_authority', 'installer_source_identity',
}
_NATIVE_FIELDS = {
    'schema', 'purpose', 'authority', 'manifest', 'manifest_identity_sha256', 'payload',
    'payload_sha256', 'manager', 'transform_abi', 'constraints', 'qualification_sha256',
}
_PAYLOAD_COMPONENTS = ('rustc', 'cargo', 'rust-std', 'clippy-preview', 'rustfmt-preview')
_PAYLOAD_PREFIXES = {
    'rustc': 'rustc-1.98.1-x86_64-unknown-linux-gnu/rustc/',
    'cargo': 'cargo-1.98.1-x86_64-unknown-linux-gnu/cargo/',
    'rust-std': 'rust-std-1.98.1-x86_64-unknown-linux-gnu/rust-std-x86_64-unknown-linux-gnu/',
    'clippy-preview': 'clippy-1.98.1-x86_64-unknown-linux-gnu/clippy-preview/',
    'rustfmt-preview': 'rustfmt-1.98.1-x86_64-unknown-linux-gnu/rustfmt-preview/',
}
_PAYLOAD_FIELDS = {'archive_member', 'component', 'mode', 'path', 'sha256', 'size'}
_CONSTRAINTS = {
    'transport': 'verified-local-file-dist-v2-only',
    'profile': 'minimal',
    'components': ['clippy', 'rustfmt'],
    'umask': '022',
    'fresh_exclusive_roots': True,
    'same_filesystem_temporary_and_toolchain': True,
    'permit_copy_rename': 'absent',
    'automatic_install': False,
    'self_update': False,
    'force': False,
    'ambient_compiler_path': False,
    'legacy_manifest_fallback': False,
    'foundation_execution_admission': 'independent-required',
}
_MANAGER = {
    'version': _MANAGER_VERSION,
    'url': 'https://static.rust-lang.org/rustup/archive/1.29.1/'
           'x86_64-unknown-linux-gnu/rustup-init',
    'sha256': _MANAGER_SHA256,
    'source_repository': 'https://github.com/rust-lang/rustup',
    'source_commit': 'd95a37b6ab92cc1e455d1576039333c97ca3e2c5',
    'source_tree': 'a346236d560c33eaaadced018b78581e3b45d245',
}
_SOURCE_ENVIRONMENT = {
    'RUNNER_TEMP': '${{ runner.temp }}',
    _ROOT_ENV: '${{ runner.temp }}/' + _NAMESPACE,
    'CARGO_HOME': '${{ runner.temp }}/' + _NAMESPACE + '/cargo-home',
    'RUSTUP_HOME': '${{ runner.temp }}/' + _NAMESPACE + '/rustup-home',
    'RUSTUP_TOOLCHAIN': _TOOLCHAIN,
    'RUSTUP_AUTO_INSTALL': '0',
}


def _reject(reason):
    raise ColdSourceIntent('root_rust_candidate_' + reason)


def _digest(value):
    return (type(value) is str and len(value) == 64
            and all(char in '0123456789abcdef' for char in value))


def _canonical(value):
    try:
        return json.dumps(value, sort_keys=True, separators=(',', ':'),
                          ensure_ascii=True, allow_nan=False).encode('ascii')
    except (TypeError, ValueError, UnicodeEncodeError) as error:
        raise ColdSourceIntent('root_rust_candidate_canonical') from error


def _sha256(value):
    return hashlib.sha256(value).hexdigest()


def _record_digest(record):
    return _sha256(_canonical(record))


def _relative(value):
    if (type(value) is not str or not value or not value.isascii()
            or value.startswith('/') or '\\' in value or '\0' in value
            or any(part in ('', '.', '..') for part in value.split('/'))):
        _reject('native_payload_path')
    return value


def _validate_payload(payload):
    if type(payload) is not list or len(payload) != 156:
        _reject('native_payload_count')
    paths, members, components = set(), set(), set()
    for item in payload:
        if type(item) is not dict or set(item) != _PAYLOAD_FIELDS:
            _reject('native_payload_record')
        component = item['component']
        path = _relative(item['path'])
        member = _relative(item['archive_member'])
        if component not in _PAYLOAD_COMPONENTS:
            _reject('native_payload_component')
        if member != _PAYLOAD_PREFIXES[component] + path:
            _reject('native_payload_archive_mapping')
        if path in paths or member in members:
            _reject('native_payload_duplicate')
        if (type(item['size']) is not int or isinstance(item['size'], bool)
                or not 0 <= item['size'] <= 512 * 1024 * 1024
                or type(item['mode']) is not int or isinstance(item['mode'], bool)
                or item['mode'] not in (0o644, 0o755) or not _digest(item['sha256'])):
            _reject('native_payload_record')
        paths.add(path)
        members.add(member)
        components.add(component)
    if components != set(_PAYLOAD_COMPONENTS):
        _reject('native_payload_components')
    if _sha256(_canonical(payload)) != _PAYLOAD_CANONICAL_SHA256:
        _reject('native_payload_identity')


def _validate_native_source(record):
    if type(record) is not dict or set(record) != _NATIVE_FIELDS:
        _reject('native_source_authority')
    if (record['schema'] != 1 or type(record['schema']) is not int
            or record['purpose'] != 'root-linux-candidate-artifact-v1'
            or record['authority'] != 'source-inputs-only'
            or record['transform_abi'] != _TRANSFORM_ABI
            or record['payload_sha256'] != _PAYLOAD_FILE_SHA256
            or record['manager'] != _MANAGER
            or record['constraints'] != _CONSTRAINTS):
        _reject('native_source_authority')
    try:
        manifest_identity = _manifest_identity(record['manifest'])
    except ColdSourceIntent as error:
        raise ColdSourceIntent('root_rust_candidate_native_manifest') from error
    if (manifest_identity != _MANIFEST_IDENTITY_SHA256
            or record['manifest_identity_sha256'] != manifest_identity):
        _reject('native_manifest_identity')
    _validate_payload(record['payload'])
    identity = dict(record)
    identity.pop('qualification_sha256')
    qualification = _sha256(_canonical(identity))
    if (record['qualification_sha256'] != qualification
            or qualification != _NATIVE_QUALIFICATION_SHA256):
        _reject('native_source_identity')


def _validate_stages(stages):
    if type(stages) is not list or len(stages) != 3:
        _reject('stage_order')
    for stage, name in zip(stages, ('clear', 'acquire', 'install')):
        if (type(stage) is not dict
                or set(stage) != {
                    'name', 'source', 'source_sha256', 'arguments', 'executable_roster',
                    'executable_search_paths',
                }
                or stage['name'] != name or type(stage['source']) is not str
                or type(stage['arguments']) is not list or stage['arguments']
                or type(stage['executable_roster']) is not list
                or type(stage['executable_search_paths']) is not list
                or stage['executable_roster'] != list(_EXECUTABLE_ROSTER)
                or stage['executable_search_paths'] != list(_EXECUTABLE_SEARCH_PATHS)
                or not _digest(stage['source_sha256'])):
            _reject('stage_source')
        try:
            source = stage['source'].encode()
        except UnicodeEncodeError as error:
            raise ColdSourceIntent('root_rust_candidate_stage_source') from error
        if (not source or len(source) > 1024 * 1024 or '${{' in stage['source']):
            _reject('stage_source')
        source_sha256 = _sha256(source)
        if source_sha256 != stage['source_sha256']:
            _reject('stage_source')


def _validate_recipe(record):
    if type(record) is not dict or set(record) != _RECIPE_FIELDS:
        _reject('recipe_unavailable')
    if (record['schema'] != 1 or type(record['schema']) is not int
            or record['purpose'] != 'root-linux-compiler-candidate'
            or record['role'] != 'root-linux' or record['host'] != _HOST
            or record['rust_version'] != _VERSION or record['toolchain'] != _TOOLCHAIN
            or record['namespace'] != _NAMESPACE or type(record['leaves']) is not list
            or tuple(record['leaves']) != _LEAVES
            or record['manager_sha256'] != _MANAGER_SHA256
            or record['manager_version'] != _MANAGER_VERSION
            or type(record['installer_source_identity']) is not str
            or not _digest(record['installer_source_identity'])):
        _reject('recipe_binding')
    if type(record['environment']) is not dict or record['environment'] != _SOURCE_ENVIRONMENT:
        _reject('environment_source')
    _validate_stages(record['stages'])
    expected_installer = _sha256('\0'.join(stage['source'] for stage in record['stages']).encode())
    if record['installer_source_identity'] != expected_installer:
        _reject('installer_identity')
    _validate_native_source(record['native_source_authority'])


def _compiled_candidate_recipe():
    """Return the exact source-owned candidate recipe, or fail closed."""
    record = _COMPILED_ROOT_RUST_CANDIDATE_RECIPE
    _validate_recipe(record)
    return json.loads(json.dumps(record, sort_keys=True, separators=(',', ':'),
                                 ensure_ascii=True, allow_nan=False))


def _candidate_environment(recipe, foundation):
    """Resolve the fixed candidate environment from the Foundation issuer only."""
    _validate_recipe(recipe)
    if type(foundation) is not FreshPreparationFoundation:
        _reject('foundation_issuer')
    foundation.require_purpose('root-linux-compiler-candidate')
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        _reject('actual_host')
    root = foundation.candidate_root
    foundation.require_candidate_root(root)
    suffix = '/' + _NAMESPACE
    if (type(root) is not str or not root.endswith(suffix)
            or not root.startswith('/') or not root.isascii()
            or any(ord(char) < 32 or ord(char) == 127 for char in root)
            or os.path.realpath(root) != root):
        _reject('foundation_root')
    temp = root[:-len(suffix)]
    if (not temp.startswith('/') or not temp.isascii() or os.path.realpath(temp) != temp
            or not os.path.isdir(temp)
            or any(ord(char) < 32 or ord(char) == 127 for char in temp)
            or any(part in ('', '.', '..') for part in temp.split('/')[1:])):
        _reject('runner_temp')
    environment = {key: value.replace('${{ runner.temp }}', temp)
                   for key, value in _SOURCE_ENVIRONMENT.items()}
    expected = {
        'RUNNER_TEMP': temp, _ROOT_ENV: root,
        'CARGO_HOME': root + '/cargo-home', 'RUSTUP_HOME': root + '/rustup-home',
        'RUSTUP_TOOLCHAIN': _TOOLCHAIN, 'RUSTUP_AUTO_INSTALL': '0',
    }
    if environment != expected:
        _reject('environment_roots')
    ca_file = foundation.ca_file
    if (type(ca_file) is not str or not ca_file.startswith('/') or not ca_file.isascii()
            or os.path.realpath(ca_file) != ca_file
            or any(ord(char) < 32 or ord(char) == 127 for char in ca_file)
            or any(part in ('', '.', '..') for part in ca_file.split('/')[1:])):
        _reject('ca_file')
    try:
        ca_mode = os.stat(ca_file, follow_symlinks=False).st_mode
    except OSError as error:
        raise ColdSourceIntent('root_rust_candidate_ca_file') from error
    if not stat.S_ISREG(ca_mode):
        _reject('ca_file')
    environment.update(PATH='/usr/bin:/bin:/usr/sbin:/sbin', LANG='C', LC_ALL='C',
                       SSL_CERT_FILE=ca_file, CURL_CA_BUNDLE=ca_file)
    return root, environment
