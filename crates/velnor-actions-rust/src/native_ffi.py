"""Fixed Rust native producer primitives; generation embeds validated inputs."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

TARGET = 'aarch64-apple-darwin'


def safe_path(root, value):
    if not isinstance(value, str) or not value or '\\' in value:
        raise ValueError('invalid Rust producer path')
    relative = Path(value)
    if relative.is_absolute() or any(part in ('', '.', '..') for part in value.split('/')):
        raise ValueError('Rust producer path must be normalized and relative')
    root = Path(root).resolve(strict=True)
    candidate = root / relative
    for parent in [candidate, *candidate.parents]:
        if parent == root:
            break
        if parent.is_symlink():
            raise ValueError('Rust producer path contains symlink')
    if not candidate.resolve().is_relative_to(root):
        raise ValueError('Rust producer path escapes source root')
    return candidate


def environment(root, deployment):
    sensitive = re.compile(r'(TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY|CREDENTIAL|CERT|ISSUER|DEVELOPER_ID|TEAM_ID)')
    prefixes = ('GITHUB_', 'ACTIONS_', 'RUNNER_', 'GH_', 'APP_STORE_', 'GIT_')
    result = {name: value for name, value in os.environ.items()
              if not name.startswith(prefixes) and not sensitive.search(name.upper())}
    result['CARGO_TARGET_DIR'] = str(safe_path(root, 'target'))
    safe_path(root, 'target/boltffi/pack/apple')
    result['MACOSX_DEPLOYMENT_TARGET'] = deployment
    return result


def run(argv, cwd, env):
    result = subprocess.run([str(arg) for arg in argv], cwd=cwd, env=env,
                            check=True, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT)
    print(result.stdout, end='')
    return result.stdout


def scalar(value, pattern):
    if not isinstance(value, str) or re.fullmatch(pattern, value) is None:
        raise ValueError('invalid Rust producer scalar')


def source(request):
    checkout = Path.cwd().resolve(strict=True)
    value = request['source_root']
    root = checkout if value == '.' else safe_path(checkout, value)
    if not root.is_dir():
        raise ValueError('missing Rust producer source root')
    scalar(request['source_sha'], r'[0-9a-f]{40}')
    if request['compile_driver'] not in ('cargo', 'mbx'):
        raise ValueError('unapproved Rust compile driver')
    env = environment(root, request.get('deployment_target', ''))
    actual = run(['/usr/bin/git', '-C', root, 'rev-parse', '--verify', 'HEAD'], root, env).strip()
    if actual != request['source_sha']:
        raise ValueError('Rust producer source differs from admitted SHA')
    return root, env


def validate_ffi(request, root):
    manifest = safe_path(root, request['manifest_path'])
    if manifest.name != 'Cargo.toml' or not manifest.is_file():
        raise ValueError('missing Rust FFI producer manifest')
    for field in ('package', 'profile'):
        scalar(request[field], r'[A-Za-z_][A-Za-z0-9_-]{0,127}')
    scalar(request['static_library'], r'lib[A-Za-z0-9_-]+\.a')
    scalar(request['deployment_target'], r'[0-9]+\.[0-9]+')
    scalar(request['profile_digest'], r'b3-[0-9a-f]{64}')
    for field in ('framework_name', 'module_name'):
        scalar(request[field], r'[A-Za-z_][A-Za-z0-9_]{0,127}')
    features = request['features']
    if not isinstance(features, list) or features != sorted(set(features)) or len(features) > 128:
        raise ValueError('invalid Rust FFI features')
    for feature in features:
        scalar(feature, r'[A-Za-z0-9_][A-Za-z0-9_.+-]{0,127}')
    return manifest


def overlay(request, stage):
    config = stage / 'boltffi.overlay.toml'
    text = ('[package]\ncrate = ' + json.dumps(request['package']) + '\n'
            '[cargo]\nglobal_args = []\n[cargo.command_args]\nbuild = []\ngenerate = []\n'
            '[targets.apple]\nenabled = true\noutput = ' + json.dumps(str(stage / 'apple-output')) + '\n'
            'deployment_target = ' + json.dumps(request['deployment_target']) + '\n'
            'include_macos = true\nios_architectures = []\nsimulator_architectures = []\n'
            'macos_architectures = ["arm64"]\n[targets.apple.xcframework]\nname = ' +
            json.dumps(request['framework_name']) + '\noutput = ' + json.dumps(str(stage / 'unused.xcframework')) + '\n'
            '[targets.apple.spm]\nlayout = "split"\nskip_package_swift = true\ndistribution = "local"\n'
            'output = ' + json.dumps(str(stage / 'spm-output')) + '\n'
            '[targets.apple.debug_symbols]\nenabled = false\nstandalone_archive = false\n'
            'output = ' + json.dumps(str(stage / 'symbols-output')) + '\n'
            '[targets.apple.swift]\nmodule_name = ' + json.dumps(request['framework_name']) + '\n'
            'ffi_module_name = ' + json.dumps(request['module_name']) + '\n'
            'output = ' + json.dumps(str(stage / 'bindings')) + '\n')
    config.write_text(text)
    return config


def boltffi(request, config, operation, root, env):
    if operation not in (['generate', 'swift'], ['build', 'apple']):
        raise ValueError('unsupported fixed BoltFFI operation')
    crate = safe_path(root, request['manifest_path']).parent
    if not safe_path(crate, 'boltffi.toml').is_file():
        raise ValueError('Rust FFI producer requires boltffi.toml')
    argv = ['boltffi', '--cargo-arg=--locked', '--cargo-arg=--profile',
            '--cargo-arg=' + request['profile']]
    for feature in request['features']:
        argv += ['--cargo-arg=--features', '--cargo-arg=' + feature]
    if operation == ['generate', 'swift']:
        argv += ['--cargo-arg=--target', '--cargo-arg=' + TARGET]
    run(argv + ['--overlay', config, *operation], crate, env)


def compile_library(request, root, env):
    manifest = safe_path(root, request['manifest_path'])
    run(['rustup', 'target', 'add', TARGET], root, env)
    argv = [request['compile_driver'], 'build', '--locked', '--manifest-path', manifest,
            '--profile', request['profile'], '--target', TARGET, '--package', request['package'],
            '--message-format=json-render-diagnostics']
    for feature in request['features']:
        argv += ['--features', feature]
    output = run(argv, root, env)
    libraries = []
    for line in output.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(message, dict) or message.get('reason') != 'compiler-artifact':
            continue
        observed = message.get('manifest_path')
        if observed != str(manifest) or 'staticlib' not in message.get('target', {}).get('crate_types', []):
            continue
        for filename in message.get('filenames', []):
            candidate = Path(filename)
            if candidate.name == request['static_library']:
                target = Path(env['CARGO_TARGET_DIR']) / TARGET
                if not candidate.is_absolute() or not candidate.is_relative_to(target):
                    raise ValueError('Rust static library artifact escapes owned target')
                if len(candidate.relative_to(target).parts) != 2:
                    raise ValueError('Rust static library artifact has unexpected target layout')
                candidate = safe_path(root, candidate.relative_to(root).as_posix())
                if not candidate.is_file():
                    raise ValueError('missing Rust static library artifact')
                libraries.append(candidate)
    if len(libraries) != 1:
        raise ValueError('Cargo must report exactly one selected static library artifact')
    return libraries[0]


def prepare_output(request):
    checkout = Path.cwd().resolve(strict=True)
    value = request['source_root']
    root = checkout if value == '.' else safe_path(checkout, value)
    scalar(request['package'], r'[A-Za-z_][A-Za-z0-9_-]{0,127}')
    output = safe_path(root, request['output_root'])
    if output.parent.name != '.velnor-rust-ffi' or output.name != request['package']:
        raise ValueError('unowned Rust FFI output destination')
    if output.exists():
        shutil.rmtree(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    return output


def produce(request, build=True):
    output = prepare_output(request)
    root, env = source(request)
    validate_ffi(request, root)
    with tempfile.TemporaryDirectory(prefix='velnor-rust-ffi-', dir=output.parent) as temporary:
        stage = Path(temporary)
        config = overlay(request, stage)
        library = compile_library(request, root, env) if build else None
        boltffi(request, config, ['generate', 'swift'], root, env)
        if build:
            boltffi(request, config, ['build', 'apple'], root, env)
        bindings = stage / 'bindings'
        if not bindings.is_dir() or bindings.is_symlink():
            raise ValueError('missing generated Rust FFI bindings')
        for item in bindings.rglob('*'):
            if item.is_symlink():
                raise ValueError('symlink in Rust FFI generated bindings')
        published = stage / 'published'
        published.mkdir()
        shutil.copytree(bindings, published / 'bindings')
        if library is not None:
            (published / 'library').mkdir()
            shutil.copyfile(library, published / 'library' / request['static_library'])
        write_artifacts(request, published, library is not None)
        published.rename(output)


def library_tests(request):
    packages = request['packages']
    if not isinstance(packages, list) or not packages or len(packages) > 128 or packages != sorted(set(packages)):
        raise ValueError('Rust native tests require explicit unique Cargo packages')
    for package in packages:
        scalar(package, r'[A-Za-z_][A-Za-z0-9_-]{0,127}')
    root, env = source(request)
    argv = [request['compile_driver'], 'nextest', 'run', '--locked', '--lib']
    for package in packages:
        argv += ['--package', package]
    run(argv, root, env)


def write_artifacts(request, published, has_library):
    bindings = published / 'bindings'
    headers = [path for path in bindings.rglob('*.h') if path.is_file()]
    if len(headers) != 1:
        raise ValueError('Rust FFI producer requires exactly one generated header')
    relative = headers[0].relative_to(published).as_posix()
    scalar(request['profile_digest'], r'b3-[0-9a-f]{64}')
    output = request['output_root']
    record = {'schema': 1, 'producer_kind': 'library' if has_library else 'bindings',
              'generated_header': output + '/' + relative,
              'header_namespace': request['package'], 'module_name': request['module_name'],
              'library_path': output + '/library/' + request['static_library'] if has_library else None,
              'bindings_path': output + '/bindings', 'source_sha': request['source_sha'],
              'profile_digest': request['profile_digest'],
              'hashes': {path.relative_to(published).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                         for path in sorted(published.rglob('*')) if path.is_file()}}
    (published / 'artifacts.json').write_text(json.dumps(record, sort_keys=True) + '\n')
