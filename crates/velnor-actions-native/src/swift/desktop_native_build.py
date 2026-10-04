"""Swift binding validation and native Xcode assembly from producer artifacts."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import tempfile

from desktop_native_core import keys, path, run, safe_path, scalar


def files(root):
    if not root.is_dir() or root.is_symlink():
        raise ValueError(f'missing or symlink directory: {root}')
    result = {}
    for item in sorted(root.rglob('*')):
        if item.is_symlink():
            raise ValueError(f'symlink in native tree: {item}')
        if item.is_file():
            result[item.relative_to(root).as_posix()] = item.read_bytes()
        elif not item.is_dir():
            raise ValueError(f'special file in native tree: {item}')
    return result


def artifact_paths(profile, record, root):
    resolved = {}
    for field in ['generated_header', 'bindings_path', 'library_path']:
        value = record[field]
        if value is None and field == 'library_path':
            resolved[field] = None
            continue
        candidate = safe_path(profile['_root'], value)
        if not candidate.is_relative_to(root) or candidate == root:
            raise ValueError('producer artifact path escapes its receipt root')
        resolved[field] = candidate
    if not resolved['bindings_path'].is_dir() or not resolved['generated_header'].is_file():
        raise ValueError('producer binding artifacts missing')
    if resolved['generated_header'].suffix != '.h':
        raise ValueError('producer header must be a C header')
    library = resolved['library_path']
    if library is not None and (not library.is_file() or library.suffix != '.a'):
        raise ValueError('producer library must be a regular static archive')
    return resolved


def load_artifacts(profile, library=False):
    receipt = profile.get('_rust_artifacts')
    expected = profile.get('_rust_profile_digest')
    scalar(expected, 'producer profile digest', r'b3-[0-9a-f]{64}')
    if not isinstance(receipt, Path):
        raise ValueError('an explicit producer artifact receipt is required')
    receipt = safe_path(profile['_root'], receipt.relative_to(profile['_root']).as_posix())
    record = json.loads(receipt.read_text())
    keys(record, ['schema', 'producer_kind', 'source_sha', 'profile_digest', 'generated_header',
                  'header_namespace', 'module_name', 'library_path', 'bindings_path', 'hashes'])
    if type(record['schema']) is not int or record['schema'] != 1:
        raise ValueError('unsupported producer artifact schema')
    scalar(record['source_sha'], 'producer source SHA', r'[0-9a-f]{40}')
    scalar(profile.get('_source_sha'), 'approved source SHA', r'[0-9a-f]{40}')
    if record['source_sha'] != profile.get('_source_sha') or record['profile_digest'] != expected:
        raise ValueError('producer artifact source or profile differs from approved inputs')
    if record['producer_kind'] not in ('bindings', 'library') or library and record['producer_kind'] != 'library':
        raise ValueError('producer artifact kind does not satisfy this stage')
    scalar(record['header_namespace'], 'header namespace')
    scalar(record['module_name'], 'foreign module', r'[A-Za-z_][A-Za-z0-9_]{0,127}')
    if record['module_name'] != profile['ffi']['module_name']:
        raise ValueError('producer module differs from Swift consumer module')
    if (record['producer_kind'] == 'library') != (record['library_path'] is not None):
        raise ValueError('producer artifact kind and library presence differ')
    resolved = artifact_paths(profile, record, receipt.parent)
    inventory = files(receipt.parent)
    inventory.pop(receipt.name)
    hashes = {name: hashlib.sha256(data).hexdigest() for name, data in inventory.items()}
    if not hashes or hashes != record['hashes']:
        raise ValueError('producer artifact inventory or bytes differ from receipt')
    if library and resolved['library_path'] is None:
        raise ValueError('producer library artifact missing')
    return dict(record, **resolved)


def normalized_bindings(artifact):
    result = {}
    for name, data in files(artifact['bindings_path']).items():
        if (artifact['bindings_path'] / name) == artifact['generated_header']:
            continue
        if name.endswith('.swift'):
            lines = [line.rstrip() for line in data.decode().splitlines()]
            while lines and not lines[-1]:
                lines.pop()
            data = ('\n'.join(lines) + ('\n' if lines else '')).encode()
        result[name] = data
    return result


def bindings_check(profile, artifact=None):
    artifact = artifact or load_artifacts(profile)
    committed = files(path(profile, 'ffi.bindings_path'))
    if not committed or committed != normalized_bindings(artifact):
        raise ValueError('committed Swift bindings differ from approved producer artifacts')


def assemble_framework(profile):
    framework = path(profile, 'ffi.xcframework_path')
    archive = safe_path(profile['_root'], framework.with_suffix('.xcframework.zip').relative_to(profile['_root']).as_posix())
    artifact = load_artifacts(profile, library=True)
    bindings_check(profile, artifact)
    for old in [framework, archive]:
        if old.exists():
            shutil.rmtree(old) if old.is_dir() else old.unlink()
    framework.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='velnor-framework-', dir=framework.parent) as temporary:
        create_framework(profile, artifact, Path(temporary), archive)
    validate_framework(profile, artifact)


def create_framework(profile, artifact, stage, archive):
    namespace = artifact['header_namespace']
    public = safe_path(stage, 'Headers/' + namespace + '/' + namespace + '.h')
    public.parent.mkdir(parents=True, exist_ok=True)
    public.write_bytes(artifact['generated_header'].read_bytes())
    modulemap = safe_path(stage, 'Headers/module.modulemap')
    modulemap.write_text('module ' + artifact['module_name'] + ' {\n    header "' +
                         namespace + '/' + namespace + '.h"\n    export *\n}\n')
    library = artifact['library_path']
    if run(['lipo', '-archs', library]).split() != ['arm64']:
        raise ValueError('producer static library must be arm64 only')
    framework = path(profile, 'ffi.xcframework_path')
    if framework.exists():
        raise ValueError('XCFramework output must be absent before assembly')
    run(['xcodebuild', '-create-xcframework', '-library', library, '-headers', modulemap.parent,
         '-output', framework], cwd=profile['_root'], stream=True)
    archive = safe_path(profile['_root'], archive.relative_to(profile['_root']).as_posix())
    if archive.exists():
        raise ValueError('XCFramework archive must be absent before assembly')
    run(['ditto', '-c', '-k', '--keepParent', framework, archive], cwd=profile['_root'])


def validate_framework(profile, artifact):
    framework = path(profile, 'ffi.xcframework_path')
    inventory = files(framework)
    module = 'macos-arm64/Headers/module.modulemap'
    if module not in inventory or ('module ' + artifact['module_name'] + ' ') not in inventory[module].decode():
        raise ValueError('XCFramework modulemap mismatch')
    libs = [framework / name for name in inventory if Path(name).name == artifact['library_path'].name]
    if len(libs) != 1 or run(['lipo', '-archs', libs[0]]).split() != ['arm64']:
        raise ValueError('XCFramework must contain exactly one arm64 static library')
    run(['plutil', '-lint', framework / 'Info.plist'])


def generate_project(profile):
    run(['xcodegen', 'generate', '--spec', path(profile, 'apple.project_spec')],
        cwd=path(profile, 'native_root'))


def xcode_arguments(profile, configuration='Release'):
    return ['xcodebuild', '-project', path(profile, 'apple.project_path'),
            '-scheme', profile['apple']['scheme'], '-configuration', configuration,
            '-destination', 'platform=macOS,arch=arm64', '-derivedDataPath',
            path(profile, 'apple.derived_data_path')]


def uuid(binary):
    import re
    output = run(['xcrun', 'dwarfdump', '--uuid', binary])
    matches = re.findall(r'^UUID: ([A-Fa-f0-9-]+) \(arm64\) ', output, re.MULTILINE)
    if len(matches) != 1 or len(output.splitlines()) != 1:
        raise ValueError('expected exactly one arm64 UUID')
    return matches[0].upper()


def build_app(profile, version, build):
    from desktop_native_verify import verify_app
    assemble_framework(profile)
    generate_project(profile)
    run(xcode_arguments(profile) + ['MARKETING_VERSION=' + version,
        'CURRENT_PROJECT_VERSION=' + build, 'ARCHS=arm64',
        'MACOSX_DEPLOYMENT_TARGET=' + profile['deployment_target'], 'build'],
        cwd=path(profile, 'native_root'), stream=True)
    app = path(profile, 'apple.app_path')
    products = safe_path(path(profile, 'apple.derived_data_path'), 'Build/Products/Release')
    app.parent.mkdir(parents=True, exist_ok=True)
    for suffix in ['', '.dSYM']:
        source = safe_path(products, app.name + suffix)
        destination = app.with_name(app.name + suffix)
        safe_path(profile['_root'], str(destination.relative_to(profile['_root'])))
        files(source)
        if destination.exists():
            shutil.rmtree(destination)
        run(['ditto', source, destination])
    name = profile['apple']['app_name']
    binary = app / 'Contents/MacOS' / name
    dwarf = app.with_name(app.name + '.dSYM') / 'Contents/Resources/DWARF' / name
    if run(['lipo', '-archs', dwarf]).split() != ['arm64'] or uuid(binary) != uuid(dwarf):
        raise ValueError('dSYM UUID or architecture differs from app')
    run(['codesign', '--force', '--sign', '-', '--timestamp=none', app])
    verify_app(profile, version, build)
