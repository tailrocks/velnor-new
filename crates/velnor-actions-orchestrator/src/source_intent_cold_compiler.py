"""Validate the source owner's native compiler projection; never mint authority."""
import hashlib

from source_intent_cold_common import ColdSourceIntent

_COMPILED_COLD_COMPILER_TRANSFORM_ABI = None


def _digest(value, length=64):
    return isinstance(value, str) and len(value) == length and all(c in '0123456789abcdef' for c in value)


def _identity(values):
    digest = hashlib.sha256()
    for value in values:
        data = value.encode('utf-8')
        digest.update(str(len(data)).encode('ascii') + b':' + data)
    return digest.hexdigest()


def require_compiler_projection(record):
    fields = {'schema', 'purpose', 'qualification_sha256', 'manifest',
              'installation_receipt_sha256', 'installed_tree_sha256', 'transform_abi'}
    if (type(record) is not dict or set(record) != fields or type(record['schema']) is not int
            or record['schema'] != 1 or record['purpose'] != 'root-linux-compiler-artifact-v1'
            or any(not _digest(record[key]) for key in ('qualification_sha256',
                       'installation_receipt_sha256', 'installed_tree_sha256'))
            or not isinstance(record['transform_abi'], str)
            or not 1 <= len(record['transform_abi']) <= 256
            or any(ord(c) < 33 or ord(c) > 126 for c in record['transform_abi'])
            or record['transform_abi'] != _COMPILED_COLD_COMPILER_TRANSFORM_ABI):
        raise ColdSourceIntent('cold_sdk_native_installation_authority_unavailable')
    manifest_identity = _manifest_identity(record['manifest'])
    expected = _identity(('velnor-root-rust-compiler-artifact-v1', manifest_identity,
        record['installation_receipt_sha256'], record['installed_tree_sha256'], record['transform_abi']))
    if record['qualification_sha256'] != expected:
        raise ColdSourceIntent('cold_sdk_native_installation_identity')


def _manifest_identity(manifest):
    fields = ('version', 'target', 'manifest_url', 'manifest_sha256', 'release_date',
              'rust_source_repository', 'rust_source_commit', 'rust_source_tree',
              'cargo_source_repository', 'cargo_source_commit', 'cargo_source_tree')
    if (type(manifest) is not dict or set(manifest) != set(fields) | {'components'}
            or manifest['version'] != '1.98.1' or manifest['target'] != 'x86_64-unknown-linux-gnu'
            or manifest['manifest_url'] != 'https://static.rust-lang.org/dist/channel-rust-1.98.1.toml'
            or not _digest(manifest['manifest_sha256'])
            or manifest['rust_source_repository'] != 'https://github.com/rust-lang/rust'
            or manifest['cargo_source_repository'] != 'https://github.com/rust-lang/cargo'
            or any(not _digest(manifest[key], 40) for key in ('rust_source_commit',
                       'rust_source_tree', 'cargo_source_commit', 'cargo_source_tree'))
            or not isinstance(manifest['release_date'], str)
            or len(manifest['release_date']) != 10
            or manifest['release_date'][4] != '-' or manifest['release_date'][7] != '-'
            or not manifest['release_date'].replace('-', '').isdigit()):
        raise ColdSourceIntent('cold_sdk_native_manifest')
    components = manifest['components']
    names = ['rustc', 'cargo', 'rust-std', 'clippy-preview', 'rustfmt-preview']
    if (type(components) is not list or any(type(item) is not dict for item in components)
            or [item.get('component') for item in components] != names):
        raise ColdSourceIntent('cold_sdk_native_components')
    values = ['velnor-root-rust-manifest-v1', *(manifest[key] for key in fields), str(len(components))]
    for item in components:
        keys = ('component', 'xz_url', 'xz_sha256', 'gzip_url', 'gzip_sha256')
        if (set(item) != set(keys) or any(not isinstance(item[key], str) for key in keys)
                or not _digest(item['xz_sha256']) or not _digest(item['gzip_sha256'])):
            raise ColdSourceIntent('cold_sdk_native_components')
        archive = item['component'].removesuffix('-preview')
        prefix = 'https://static.rust-lang.org/dist/' + manifest['release_date'] + '/'
        prefix += archive + '-1.98.1-x86_64-unknown-linux-gnu.tar.'
        if item['xz_url'] != prefix + 'xz' or item['gzip_url'] != prefix + 'gz':
            raise ColdSourceIntent('cold_sdk_native_component_url')
        values.extend(item[key] for key in keys)
    return _identity(values)
