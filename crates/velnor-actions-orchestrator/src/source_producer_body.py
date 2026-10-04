"""Closed native Cargo fetch owner; the sole argument is an admitted descriptor."""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import time
import tomllib
from enum import Enum
IDENTITY = 'index.crates.io-1949cf8c6b5b557f'
REGISTRY = 'registry+https://github.com/rust-lang/crates.io-index'
REGISTRIES = {REGISTRY, 'sparse+https://index.crates.io/'}
class SourceProducerError(Enum):
    DESCRIPTOR = 'source_descriptor'
    PATH = 'source_path'
    CACHE = 'source_cache'
    TOOLCHAIN = 'source_toolchain'
    FETCH = 'source_fetch'
class Failure(Exception):
    def __init__(self, reason):
        self.reason = reason
def require(condition, reason):
    if not condition:
        raise Failure(reason)
def relative(value, empty=False):
    require(isinstance(value, str), SourceProducerError.DESCRIPTOR)
    if value == '' and empty:
        return value
    path = PurePosixPath(value)
    require(value and not path.is_absolute() and '\\' not in value
            and all(part not in ('', '.', '..') for part in value.split('/')),
            SourceProducerError.PATH)
    return value
def no_links(root):
    if not root.exists() and not root.is_symlink():
        return
    require(stat.S_ISDIR(root.lstat().st_mode), SourceProducerError.PATH)
    for directory, dirs, files in os.walk(root, followlinks=False):
        for name in dirs + files:
            mode = (Path(directory) / name).lstat().st_mode
            require(stat.S_ISDIR(mode) or stat.S_ISREG(mode), SourceProducerError.PATH)
def safe_root(value):
    root = Path(value)
    require(root.is_absolute(), SourceProducerError.PATH)
    for parent in [*reversed(root.parents), root]:
        require(parent.exists() and stat.S_ISDIR(parent.lstat().st_mode),
                SourceProducerError.PATH)
    return root
def descriptor(encoded):
    try:
        data = json.loads(bytes.fromhex(encoded))
        require(isinstance(data, dict), SourceProducerError.DESCRIPTOR)
        fields = {'schema', 'rust_version', 'target', 'roots', 'manifests',
                  'locks', 'archives', 'mode'}
        selected = data.get('mode') == 'native-tree-selected-containing'
        require(set(data) == fields | ({'selections'} if selected else set()),
                SourceProducerError.DESCRIPTOR)
        require(data['schema'] == 1 and (selected or data['mode'] == 'complete-locked-workspace'),
                SourceProducerError.DESCRIPTOR)
        require(re.fullmatch(r'\d+\.\d+\.\d+', data['rust_version'])
                and re.fullmatch(r'[A-Za-z0-9_-]+', data['target']),
                SourceProducerError.DESCRIPTOR)
        require(data['roots'] and len(set(data['roots'])) == len(data['roots']),
                SourceProducerError.DESCRIPTOR)
        for root in data['roots']:
            relative(root, empty=True)
        data['manifests'] = pairs(data['manifests'], False)
        data['locks'] = pairs(data['locks'], True)
        require(set(data['locks']) == set(data['roots']), SourceProducerError.DESCRIPTOR)
        for root in data['roots']:
            require(str(PurePosixPath(root) / 'Cargo.toml') in data['manifests'],
                    SourceProducerError.DESCRIPTOR)
        data['archives'] = archive_map(data['archives'])
        require(lock_archives(data['locks']) == data['archives'], SourceProducerError.DESCRIPTOR)
        if selected:
            validate_selections(data)
        return data
    except (ValueError, TypeError, KeyError, UnicodeError):
        raise Failure(SourceProducerError.DESCRIPTOR) from None
def pairs(values, empty):
    require(isinstance(values, list), SourceProducerError.DESCRIPTOR)
    result = {}
    for pair in values:
        require(isinstance(pair, list) and len(pair) == 2,
                SourceProducerError.DESCRIPTOR)
        path, content = pair
        relative(path, empty=empty)
        require(path not in result and isinstance(content, str), SourceProducerError.DESCRIPTOR)
        if not empty:
            require(PurePosixPath(path).name == 'Cargo.toml', SourceProducerError.DESCRIPTOR)
        result[path] = content
    return result

def validate_selections(data):
    selections = data['selections']
    require(isinstance(selections, list) and bool(selections), SourceProducerError.DESCRIPTOR)
    for selection in selections:
        require(isinstance(selection, dict) and set(selection) ==
                {'root', 'package', 'target', 'features', 'default_features'},
                SourceProducerError.DESCRIPTOR)
        require(selection['root'] in data['roots']
                and isinstance(selection['package'], str)
                and re.fullmatch(r'[A-Za-z0-9_-]+', selection['package'])
                and isinstance(selection['default_features'], bool), SourceProducerError.DESCRIPTOR)
        target = selection['target']
        require(target is None or (isinstance(target, str) and
                re.fullmatch(r'[A-Za-z0-9_-]+', target)), SourceProducerError.DESCRIPTOR)
        require(isinstance(selection['features'], list) and all(isinstance(feature, str)
                and re.fullmatch(r'[A-Za-z0-9_+-]+(?:/[A-Za-z0-9_+-]+)?', feature)
                for feature in selection['features']), SourceProducerError.DESCRIPTOR)

def archive_map(values):
    result = {}
    for name, version, checksum in values:
        require(re.fullmatch(r'[A-Za-z0-9_-]+', name)
                and re.fullmatch(r'\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?(?:\+[A-Za-z0-9.-]+)?', version)
                and re.fullmatch(r'[a-f0-9]{64}', checksum), SourceProducerError.DESCRIPTOR)
        filename = name + '-' + version + '.crate'
        require(filename not in result, SourceProducerError.DESCRIPTOR)
        result[filename] = checksum
    return result

def lock_archives(locks):
    result = {}
    for content in locks.values():
        for package in tomllib.loads(content).get('package', []):
            if 'source' in package:
                require(package['source'] in REGISTRIES, SourceProducerError.DESCRIPTOR)
                item = archive_map([[package['name'], package['version'], package['checksum']]])
                for name, checksum in item.items():
                    require(name not in result or result[name] == checksum,
                            SourceProducerError.DESCRIPTOR)
                    result[name] = checksum
    return result

def target_paths(manifest_path, content):
    doc = tomllib.loads(content)
    base = PurePosixPath(manifest_path).parent
    outputs = set()
    if 'package' in doc:
        outputs.update(str(base / path) for path in ('src/lib.rs', 'src/main.rs'))
        build = doc['package'].get('build', 'build.rs')
        if build is not False:
            relative(build)
            outputs.add(str(base / build))
    sections = [('lib', doc.get('lib', {}))]
    for kind in ('bin', 'test', 'example', 'bench'):
        sections.extend((kind, section) for section in doc.get(kind, []))
    for kind, section in sections:
        if 'path' in section:
            relative(section['path'])
            outputs.add(str(base / section['path']))
        elif kind != 'lib' and 'name' in section:
            name = section['name']
            require(isinstance(name, str) and re.fullmatch(r'[A-Za-z0-9_-]+', name),
                    SourceProducerError.DESCRIPTOR)
            folder = {'bin': 'src/bin', 'test': 'tests', 'example': 'examples',
                      'bench': 'benches'}[kind]
            path = folder + '/' + name + '.rs'
            if kind == 'bin' and name == doc.get('package', {}).get('name'):
                path = 'src/main.rs'
            outputs.add(str(base / path))
    check_dependencies(doc, base)
    return outputs

def check_dependencies(value, base):
    if isinstance(value, dict):
        require('git' not in value, SourceProducerError.DESCRIPTOR)
        if 'registry' in value and isinstance(value['registry'], str):
            require(value['registry'] == 'crates-io', SourceProducerError.DESCRIPTOR)
        for key, child in value.items():
            if key == 'path' and isinstance(child, str):
                confined_dependency(child, base)
            check_dependencies(child, base)
    elif isinstance(value, list):
        for child in value:
            check_dependencies(child, base)

def confined_dependency(value, base):
    require(value and not PurePosixPath(value).is_absolute() and '\\' not in value,
            SourceProducerError.PATH)
    parts = list(base.parts)
    for part in value.split('/'):
        if part == '..':
            require(bool(parts), SourceProducerError.PATH)
            parts.pop()
        elif part not in ('', '.'):
            parts.append(part)
    require('.cargo' not in parts, SourceProducerError.PATH)

def materialize(home, data):
    root = home / 'rust-source-producer'
    no_links(root)
    if root.exists():
        shutil.rmtree(root)
    project = root / 'project'
    project.mkdir(parents=True)
    for ancestor in [project, *project.parents]:
        for name in ('config', 'config.toml'):
            config = ancestor / '.cargo' / name
            require(not config.exists() and not config.is_symlink(), SourceProducerError.PATH)
    captured = dict(data['manifests'])
    for directory, content in data['locks'].items():
        path = str(PurePosixPath(directory) / 'Cargo.lock')
        require(path not in captured, SourceProducerError.DESCRIPTOR)
        captured[path] = content
    targets = set()
    for path, content in data['manifests'].items():
        targets.update(target_paths(path, content))
    require(not targets.intersection(captured), SourceProducerError.PATH)
    for path in [*captured, *targets]:
        require('.cargo' not in PurePosixPath(path).parts, SourceProducerError.PATH)
        require(not any(other != path and path.startswith(other + '/')
                        for other in [*captured, *targets]), SourceProducerError.PATH)
    for path, content in captured.items():
        destination = project / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(content, encoding='utf-8')
    for path in targets:
        destination = project / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text('fn main() {}\n', encoding='utf-8')
    return project, root

def cache_validate(cargo_home, archives, repair):
    no_links(cargo_home)
    cargo_home.mkdir(parents=True, exist_ok=True)
    for name in ('config', 'config.toml', 'credentials', 'credentials.toml'):
        require(not (cargo_home / name).exists(), SourceProducerError.CACHE)
    git = cargo_home / 'git'
    require(not git.exists() or not any(git.iterdir()), SourceProducerError.CACHE)
    registry = cargo_home / 'registry'
    if not registry.exists():
        return {}
    for section in ('cache', 'index', 'src'):
        directory = registry / section
        if directory.exists():
            require(all(child.name == IDENTITY for child in directory.iterdir()),
                    SourceProducerError.CACHE)
    config = registry / 'index' / IDENTITY / 'config.json'
    if config.exists():
        require(json.loads(config.read_text()) ==
                {'dl': 'https://static.crates.io/crates', 'api': 'https://crates.io'},
                SourceProducerError.CACHE)
    sparse_layout(registry / 'index' / IDENTITY, archives, repair)
    cache = registry / 'cache' / IDENTITY
    valid = {}
    if cache.exists():
        for archive in cache.iterdir():
            require(archive.is_file(), SourceProducerError.CACHE)
            expected = archives.get(archive.name)
            if expected and hashlib.sha256(archive.read_bytes()).hexdigest() == expected:
                valid[archive.name] = archive.stat().st_size
            elif repair:
                archive.unlink()
            else:
                raise Failure(SourceProducerError.CACHE)
    return valid

def sparse_layout(index, archives, repair):
    if not index.exists():
        return
    allowed = {'config.json'}
    for archive in archives:
        name = re.fullmatch(r'([A-Za-z0-9_-]+)-[0-9]+\..+', archive).group(1).lower()
        prefix = str(len(name)) if len(name) < 3 else (
            '3/' + name[0] if len(name) == 3 else name[:2] + '/' + name[2:4])
        allowed.add('.cache/' + prefix + '/' + name)
    for path in sorted(index.rglob('*'), reverse=True):
        relative_path = path.relative_to(index).as_posix()
        expected = relative_path in allowed or (path.is_dir() and any(
            item.startswith(relative_path + '/') for item in allowed))
        if not expected:
            require(repair, SourceProducerError.CACHE)
            if path.is_dir():
                path.rmdir()
            else:
                path.unlink()

def environment(home, root, data):
    toolchain = data['rust_version'] + '-' + data['target']
    binary = home / 'rustup' / 'toolchains' / toolchain / 'bin'
    env = {'HOME': str(root / 'home'), 'CARGO_HOME': str(home / 'cargo'),
           'RUSTUP_HOME': str(home / 'rustup'), 'RUSTUP_TOOLCHAIN': toolchain,
           'PATH': str(binary) + ':/usr/bin:/bin', 'RUSTC': str(binary / 'rustc'),
           'CARGO_AUTO_CLEAN_FREQUENCY': 'never'}
    for name in ('HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_DATA_HOME'):
        env.setdefault(name, str(root / name.lower()))
        Path(env[name]).mkdir(parents=True, exist_ok=True)
    return binary, env

def child(command, env, cwd):
    return subprocess.run(command, env=env, cwd=cwd, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)

def toolchain_validate(binary, env, project, data):
    rustc = child([str(binary / 'rustc'), '-vV'], env, project)
    cargo = child([str(binary / 'cargo'), '-V'], env, project)
    require(rustc.returncode == 0 and cargo.returncode == 0, SourceProducerError.TOOLCHAIN)
    rust_text = rustc.stdout.decode('utf-8')
    cargo_text = cargo.stdout.decode('utf-8')
    require(('release: ' + data['rust_version']) in rust_text.splitlines()
            and ('host: ' + data['target']) in rust_text.splitlines()
            and cargo_text.startswith('cargo ' + data['rust_version'] + ' '),
            SourceProducerError.TOOLCHAIN)

def fetch(binary, env, project, data, offline):
    success = True
    operations = data.get('selections', [{'root': root} for root in data['roots']])
    for operation in operations:
        selected = 'package' in operation
        command = [str(binary / 'cargo'), 'tree' if selected else 'fetch', '--locked',
                   '--manifest-path', str(project / operation['root'] / 'Cargo.toml')]
        if selected:
            command.extend(['-p', operation['package'], '-e', 'normal,build,dev'])
            if operation['target']:
                command.extend(['--target', operation['target']])
            if not operation['default_features']:
                command.append('--no-default-features')
            if operation['features']:
                command.extend(['--features', ','.join(operation['features'])])
        if offline:
            command.append('--offline')
        if child(command, env, project).returncode != 0:
            success = False
    return success

def produce(data):
    home = safe_root(os.environ['RUNNER_TEMP']) / 'velnor'
    if home.exists():
        require(stat.S_ISDIR(home.lstat().st_mode), SourceProducerError.PATH)
    home.mkdir(exist_ok=True)
    no_links(home / 'rustup')
    no_links(home / 'cargo')
    for name in ('index', 'src'):
        untrusted = home / 'cargo' / 'registry' / name
        if untrusted.exists():
            shutil.rmtree(untrusted)
    before = cache_validate(home / 'cargo', data['archives'], True)
    project, root = materialize(home, data)
    binary, env = environment(home, root, data)
    toolchain_validate(binary, env, project, data)
    start = time.monotonic()
    require(fetch(binary, env, project, data, False), SourceProducerError.FETCH)
    refill_elapsed = int((time.monotonic() - start) * 1000)
    after = cache_validate(home / 'cargo', data['archives'], False)
    if data['mode'] == 'complete-locked-workspace':
        require(set(after) == set(data['archives']), SourceProducerError.CACHE)
    require(fetch(binary, env, project, data, True), SourceProducerError.FETCH)
    final = cache_validate(home / 'cargo', data['archives'], False)
    require(final == after, SourceProducerError.CACHE)
    print('source_scope=' + data['mode'].replace('-', '_'))
    print('source_fetch_elapsed_ms=' + str(int((time.monotonic() - start) * 1000)))
    print('source_untrusted_refill_elapsed_ms=' + str(refill_elapsed))
    print('source_downloaded_archive_bytes=unknown')
    print('source_added_archive_bytes=' + str(sum(size for name, size in after.items() if name not in before)))

def outputs(verified, error):
    path = os.environ.get('GITHUB_OUTPUT')
    if path:
        with open(path, 'a', encoding='utf-8') as stream:
            stream.write('verified=' + str(verified).lower() + '\nerror=' + error + '\n')

def main():
    try:
        require(len(sys.argv) == 2 and bool(os.environ.get('VELNOR_SOURCE_IDENTITY')),
                SourceProducerError.DESCRIPTOR)
        produce(descriptor(sys.argv[1]))
        outputs(True, 'NONE')
        return 0
    except Failure as error:
        reason = error.reason.value
    except (OSError, ValueError, TypeError, KeyError, UnicodeError):
        reason = SourceProducerError.CACHE.value
    print(reason, file=sys.stderr)
    try:
        outputs(False, reason)
    except OSError:
        pass
    return 1

if __name__ == '__main__':
    sys.exit(main())
