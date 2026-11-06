# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

from dataclasses import dataclass
import fcntl
import hashlib
import json
import os
from pathlib import PurePosixPath
import re
import stat
import struct
import time
import tomllib


class SourceExecutorError(Exception):
    pass


@dataclass(frozen=True, slots=True)
class Package:
    name: str
    version: str | None
    manifest: str | None
    features: tuple[str, ...]
    upload_config_sha256: str


@dataclass(frozen=True, slots=True)
class SourceFile:
    path: str
    sha256: str
    executable: bool | None
    target: str | None


@dataclass(frozen=True, slots=True)
class SourceIdentity:
    repository: str
    repository_id: int
    owner_id: int
    run_id: int
    run_attempt: int
    workflow_sha: str
    intent_sha256: str


@dataclass(frozen=True, slots=True)
class SealedSourceContext:
    identity: SourceIdentity
    source_sha: str
    rust_version: str
    manifest: str
    authentication: str
    source_files: tuple[SourceFile, ...]
    packages: tuple[Package, ...]
    tools: tuple[tuple[str, str], ...]


@dataclass(frozen=True, slots=True)
class SemverBaseline:
    source_sha: str
    package: str
    target_version: str
    manifest: str
    features: tuple[str, ...]
    version: str
    checksum: str
    files: tuple[SourceFile, ...]


PACKAGE_WRAPPER = '''import os,pathlib,stat,subprocess,sys
cargo,manifest,name,version,features,verify=sys.argv[1:]
args=[cargo,"package","--locked","--manifest-path",manifest,"--package",name,"--target-dir","/tmp/velnor-target"]
if features:
 args.extend(["--features",features])
if verify=="false":
 args.append("--no-verify")
subprocess.run(args,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
path="/tmp/velnor-target/package/"+name+"-"+version+".crate"
fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC)
try:
 info=os.fstat(fd)
 if not stat.S_ISREG(info.st_mode) or info.st_nlink!=1 or not 0<info.st_size<64*1024*1024:
  raise ValueError("invalid package archive")
 while True:
  part=os.read(fd,65536)
  if not part: break
  view=memoryview(part)
  while view:
   size=os.write(1,view)
   if size<=0: raise ValueError("output failed")
   view=view[size:]
finally:
 os.close(fd)
'''


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise SourceExecutorError('duplicate context field')
        result[key] = value
    return result


def valid_repository(value):
    if not isinstance(value, str) or value.count('/') != 1:
        return False
    owner, name = value.split('/')
    return (0 < len(owner) <= 39 and not owner.startswith('-') and not owner.endswith('-')
            and '--' not in owner and re.fullmatch(r'[A-Za-z0-9-]+', owner) is not None
            and 0 < len(name) <= 100 and name not in ('.', '..') and not name.endswith('.git')
            and re.fullmatch(r'[A-Za-z0-9_.-]+', name) is not None)


def parse_inventory(source_files):
    if not isinstance(source_files, list) or not 0 < len(source_files) <= 100_000:
        raise SourceExecutorError('invalid source inventory')
    inventory = []
    last = ''
    for item in source_files:
        if (not isinstance(item, dict) or set(item) != {'path', 'sha256', 'kind'}
                or not isinstance(item['path'], str) or not item['path'] or len(item['path']) > 1024
                or item['path'] <= last or item['path'].startswith('/') or '\0' in item['path']
                or any(part in ('', '.', '..') for part in item['path'].split('/'))
                or item['path'].split('/')[0] == '.git'
                or not isinstance(item['sha256'], str) or not re.fullmatch(r'[a-f0-9]{64}', item['sha256'])
                or not isinstance(item['kind'], dict)):
            raise SourceExecutorError('invalid source inventory')
        kind = item['kind']
        if set(kind) == {'type', 'executable'} and kind['type'] == 'regular' and type(kind['executable']) is bool:
            inventory.append(SourceFile(item['path'], item['sha256'], kind['executable'], None))
        elif (set(kind) == {'type', 'target'} and kind['type'] == 'symlink'
              and isinstance(kind['target'], str) and kind['target'] and '\0' not in kind['target']):
            inventory.append(SourceFile(item['path'], item['sha256'], None, kind['target']))
        else:
            raise SourceExecutorError('invalid source inventory')
        last = item['path']
    return tuple(inventory)


def read_sealed_json(descriptor):
    required = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
    info = os.fstat(descriptor)
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or not 0 < info.st_size <= 32 * 1024 * 1024
            or fcntl.fcntl(descriptor, fcntl.F_GET_SEALS) & required != required):
        raise SourceExecutorError('context is not sealed')
    value = os.pread(descriptor, info.st_size + 1, 0)
    if len(value) != info.st_size:
        raise SourceExecutorError('invalid context size')
    try:
        context = json.loads(value, object_pairs_hook=unique_object)
    except (ValueError, UnicodeError):
        raise SourceExecutorError('invalid context') from None
    return context


def read_context(descriptor):
    context = read_sealed_json(descriptor)
    if (not isinstance(context, dict) or set(context) != {'identity', 'source_sha', 'rust_version', 'manifest', 'authentication', 'source_files', 'packages', 'tools'}
            or not isinstance(context['source_sha'], str)
            or not re.fullmatch(r'[a-f0-9]{40}', context['source_sha'])
            or not isinstance(context['rust_version'], str) or len(context['rust_version']) > 32
            or not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', context['rust_version'])
            or context['authentication'] not in ('trusted-publishing', 'bootstrap-token')):
        raise SourceExecutorError('invalid context')
    identity = context['identity']
    if (not isinstance(identity, dict)
            or set(identity) != {'repository', 'repository_id', 'owner_id', 'run_id', 'run_attempt',
                                 'workflow_sha', 'intent_sha256'}
            or not valid_repository(identity['repository'])
            or not all(type(identity[name]) is int and 0 < identity[name] < 2**64
                       for name in ('repository_id', 'owner_id', 'run_id'))
            or type(identity['run_attempt']) is not int or not 0 < identity['run_attempt'] < 2**32
            or not isinstance(identity['workflow_sha'], str)
            or not re.fullmatch(r'[a-f0-9]{40}', identity['workflow_sha'])
            or not isinstance(identity['intent_sha256'], str)
            or not re.fullmatch(r'[a-f0-9]{64}', identity['intent_sha256'])):
        raise SourceExecutorError('invalid source identity')
    source_identity = SourceIdentity(**identity)
    manifest = context['manifest']
    if (not isinstance(manifest, str) or not manifest or '\0' in manifest or manifest.startswith('/')
            or any(component in ('', '.', '..') for component in manifest.split('/'))
            or PurePosixPath(manifest).name != 'Cargo.toml'):
        raise SourceExecutorError('invalid manifest')
    inventory = parse_inventory(context['source_files'])
    packages = context['packages']
    if not isinstance(packages, list) or not 0 < len(packages) <= 256:
        raise SourceExecutorError('invalid packages')
    names = set()
    for package in packages:
        if (not isinstance(package, dict) or set(package) != {'name', 'version', 'manifest', 'features', 'upload_config_sha256'}
                or not isinstance(package['name'], str)
                or len(package['name']) > 64
                or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_-]*', package['name'])
                or package['name'] in names
                or (package['version'] is not None and (not isinstance(package['version'], str)
                    or not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?(?:\+[A-Za-z0-9.-]+)?', package['version'])))
                or ((package['version'] is None) != (package['manifest'] is None))
                or (package['manifest'] is not None and (not isinstance(package['manifest'], str)
                    or package['manifest'].startswith('/') or '\0' in package['manifest']
                    or any(part in ('', '.', '..') for part in package['manifest'].split('/'))
                    or PurePosixPath(package['manifest']).name != 'Cargo.toml'
                    or package['manifest'] not in {item.path for item in inventory}))
                or not isinstance(package['features'], list) or len(package['features']) > 256
                or not all(isinstance(feature, str) and re.fullmatch(r'[A-Za-z0-9_+./:?-]+', feature)
                           for feature in package['features'])
                or not isinstance(package['upload_config_sha256'], str)
                or not re.fullmatch(r'[a-f0-9]{64}', package['upload_config_sha256'])):
            raise SourceExecutorError('invalid package')
        names.add(package['name'])
    tools = context['tools']
    if (not isinstance(tools, dict) or set(tools) != {'cargo', 'rustc', 'rustdoc', 'semver', 'git', 'git_http', 'cc', 'release_plz'}
            or not all(isinstance(value, str) and re.fullmatch(r'[a-f0-9]{64}', value)
                       for value in tools.values())):
        raise SourceExecutorError('invalid tools')
    return SealedSourceContext(
        source_identity, context['source_sha'], context['rust_version'], manifest, context['authentication'], tuple(inventory),
        tuple(Package(package['name'], package['version'], package['manifest'], tuple(package['features']), package['upload_config_sha256']) for package in packages),
        tuple((name, tools[name]) for name in sorted(tools)),
    )


def read_semver_context(descriptor):
    value = read_sealed_json(descriptor)
    fields = {'source_sha', 'package', 'target_version', 'manifest', 'features',
              'baseline_version', 'baseline_checksum', 'files'}
    version = r'[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?(?:\+[A-Za-z0-9.-]+)?'
    if (not isinstance(value, dict) or set(value) != fields
            or not isinstance(value['source_sha'], str) or not re.fullmatch(r'[a-f0-9]{40}', value['source_sha'])
            or not isinstance(value['package'], str) or len(value['package']) > 64
            or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_-]*', value['package'])
            or not isinstance(value['target_version'], str) or not re.fullmatch(version, value['target_version'])
            or not isinstance(value['baseline_version'], str) or not re.fullmatch(version, value['baseline_version'])
            or not isinstance(value['baseline_checksum'], str) or not re.fullmatch(r'[a-f0-9]{64}', value['baseline_checksum'])
            or not isinstance(value['manifest'], str) or not value['manifest'] or value['manifest'].startswith('/')
            or any(part in ('', '.', '..') for part in value['manifest'].split('/'))
            or PurePosixPath(value['manifest']).name != 'Cargo.toml'
            or not isinstance(value['features'], list) or len(value['features']) > 256
            or not all(isinstance(item, str) and re.fullmatch(r'[A-Za-z0-9_+./:?-]+', item)
                       for item in value['features'])):
        raise SourceExecutorError('invalid semver context')
    files = parse_inventory(value['files'])
    if (any(item.target is not None for item in files)
            or 'Cargo.toml' not in {item.path for item in files}):
        raise SourceExecutorError('invalid semver context')
    return SemverBaseline(value['source_sha'], value['package'], value['target_version'], value['manifest'],
                          tuple(value['features']), value['baseline_version'], value['baseline_checksum'], files)


def verify_semver_baseline(root, baseline, deadline):
    parent = os.path.join(root, 'baselines')
    path = os.path.join(parent, baseline.checksum)
    for directory in (parent, path):
        try:
            info = os.stat(directory, follow_symlinks=False)
        except OSError:
            raise SourceExecutorError('invalid baseline custody') from None
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise SourceExecutorError('invalid baseline custody')
    if os.path.lexists(os.path.join(path, '.git')):
        raise SourceExecutorError('invalid baseline custody')
    verify_immutable_tree(path, deadline, False)
    verify_source_inventory(path, baseline.files, deadline)
    manifest = os.open(os.path.join(path, 'Cargo.toml'), os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        info = os.fstat(manifest)
        if not stat.S_ISREG(info.st_mode) or info.st_size > 4 * 1024 * 1024:
            raise SourceExecutorError('invalid baseline manifest')
        data = os.read(manifest, info.st_size + 1)
        if len(data) != info.st_size:
            raise SourceExecutorError('invalid baseline manifest')
    finally:
        os.close(manifest)
    try:
        package = tomllib.loads(data.decode('utf-8'))['package']
    except (ValueError, UnicodeError, KeyError, TypeError):
        raise SourceExecutorError('invalid baseline manifest') from None
    if not isinstance(package, dict) or package.get('name') != baseline.package or package.get('version') != baseline.version:
        raise SourceExecutorError('invalid baseline manifest')
    if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
        raise SourceExecutorError('baseline deadline')
    return path


def verify_tool(path, expected, deadline):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022
                or not info.st_mode & 0o111 or not 0 < info.st_size <= 256 * 1024 * 1024):
            raise SourceExecutorError('invalid tool')
        digest = hashlib.sha256()
        total = 0
        while part := os.read(fd, 1024 * 1024):
            if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
                raise SourceExecutorError('tool verification deadline')
            total += len(part)
            digest.update(part)
        after = os.fstat(fd)
        if (total != info.st_size or (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)
                != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns)
                or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline or digest.hexdigest() != expected):
            raise SourceExecutorError('tool mismatch')
    finally:
        os.close(fd)


def resolve_host_compiler():
    pending = ['usr', 'bin', 'cc']
    resolved = []
    links = 0
    try:
        root_info = os.stat('/', follow_symlinks=False)
        if not stat.S_ISDIR(root_info.st_mode) or root_info.st_uid != 0 or root_info.st_mode & 0o022:
            raise SourceExecutorError('host compiler custody invalid')
        while pending:
            part = pending.pop(0)
            if part in ('', '.'):
                continue
            if part == '..':
                if not resolved:
                    raise SourceExecutorError('host compiler custody invalid')
                resolved.pop()
                continue
            candidate = '/' + '/'.join(resolved + [part])
            info = os.stat(candidate, follow_symlinks=False)
            if info.st_uid != 0:
                raise SourceExecutorError('host compiler custody invalid')
            if stat.S_ISLNK(info.st_mode):
                links += 1
                if links > 40:
                    raise SourceExecutorError('host compiler custody invalid')
                target = os.readlink(candidate)
                if target.startswith('/'):
                    resolved = []
                pending = target.split('/') + pending
                continue
            if pending:
                if not stat.S_ISDIR(info.st_mode) or info.st_mode & 0o022:
                    raise SourceExecutorError('host compiler custody invalid')
            elif not stat.S_ISREG(info.st_mode) or info.st_mode & 0o022 or not info.st_mode & 0o111:
                raise SourceExecutorError('host compiler custody invalid')
            resolved.append(part)
        if not resolved:
            raise SourceExecutorError('host compiler custody invalid')
        return '/' + '/'.join(resolved)
    except OSError:
        raise SourceExecutorError('host compiler custody invalid') from None


def verify_host_compiler(expected, deadline):
    path = resolve_host_compiler()
    verify_tool(path, expected, deadline)
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            header = os.read(descriptor, 20)
        finally:
            os.close(descriptor)
    except OSError:
        raise SourceExecutorError('host compiler invalid') from None
    if (len(header) != 20 or header[:6] != b'\x7fELF\x02\x01'
            or header[18:20] != b'\x3e\x00' or resolve_host_compiler() != path
            or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline):
        raise SourceExecutorError('host compiler invalid')
    return path


def verify_immutable_tree(root, deadline, allow_links):
    entries = 0
    total = 0
    def fail(error):
        raise SourceExecutorError('invalid source custody') from error
    for current, directories, files in os.walk(root, topdown=True, onerror=fail, followlinks=False):
        for name in directories + files:
            if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
                raise SourceExecutorError('source custody deadline')
            entries += 1
            if entries > 100_000:
                raise SourceExecutorError('invalid source custody')
            path = os.path.join(current, name)
            info = os.stat(path, follow_symlinks=False)
            if info.st_uid != 0:
                raise SourceExecutorError('invalid source custody')
            if stat.S_ISLNK(info.st_mode):
                if not allow_links or os.path.commonpath((root, os.path.realpath(path))) != root:
                    raise SourceExecutorError('invalid source custody')
            elif stat.S_ISDIR(info.st_mode):
                if info.st_mode & 0o022:
                    raise SourceExecutorError('invalid source custody')
            elif stat.S_ISREG(info.st_mode):
                if info.st_mode & 0o022 or info.st_nlink != 1:
                    raise SourceExecutorError('invalid source custody')
                total += info.st_size
                if total > 4 * 1024 * 1024 * 1024:
                    raise SourceExecutorError('invalid source custody')
            else:
                raise SourceExecutorError('invalid source custody')


def verify_source_inventory(root, inventory, deadline):
    expected = {item.path: item for item in inventory}
    directories = set()
    for item in inventory:
        parent = PurePosixPath(item.path).parent
        while str(parent) != '.':
            directories.add(str(parent))
            parent = parent.parent
    observed = set()
    seen_directories = set()
    total = 0
    def fail(error):
        raise SourceExecutorError('source inventory mismatch') from error
    for current, children, files in os.walk(root, topdown=True, onerror=fail, followlinks=False):
        if current == root:
            children[:] = [name for name in children if name != '.git']
            files = [name for name in files if name != '.git']
        for name in children + files:
            if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
                raise SourceExecutorError('source inventory deadline')
            path = os.path.join(current, name)
            relative = os.path.relpath(path, root)
            info = os.stat(path, follow_symlinks=False)
            if stat.S_ISDIR(info.st_mode):
                seen_directories.add(relative)
                continue
            item = expected.get(relative)
            if item is None or relative in observed:
                raise SourceExecutorError('source inventory mismatch')
            observed.add(relative)
            if stat.S_ISLNK(info.st_mode):
                target = os.readlink(path)
                if item.target != target or hashlib.sha256(target.encode('utf-8')).hexdigest() != item.sha256:
                    raise SourceExecutorError('source inventory mismatch')
                continue
            if not stat.S_ISREG(info.st_mode) or item.executable is None or bool(info.st_mode & 0o111) != item.executable:
                raise SourceExecutorError('source inventory mismatch')
            fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
            try:
                before = os.fstat(fd)
                if not stat.S_ISREG(before.st_mode) or before.st_size > 256 * 1024 * 1024:
                    raise SourceExecutorError('source inventory mismatch')
                digest = hashlib.sha256()
                size = 0
                while part := os.read(fd, 1024 * 1024):
                    if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
                        raise SourceExecutorError('source inventory deadline')
                    size += len(part)
                    digest.update(part)
                after = os.fstat(fd)
                if (size != before.st_size or digest.hexdigest() != item.sha256
                        or (info.st_dev, info.st_ino) != (before.st_dev, before.st_ino)
                        or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns)
                        != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns)):
                    raise SourceExecutorError('source inventory mismatch')
                total += size
                if total > 2 * 1024 * 1024 * 1024:
                    raise SourceExecutorError('source inventory mismatch')
            finally:
                os.close(fd)
    if observed != set(expected) or seen_directories != directories:
        raise SourceExecutorError('source inventory mismatch')


def verify_detached_head(root, expected):
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    source = os.open(root, flags)
    try:
        git = os.open('.git', flags, dir_fd=source)
        try:
            descriptor = os.open('HEAD', os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=git)
            try:
                info = os.fstat(descriptor)
                if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022
                        or info.st_nlink != 1 or info.st_size != 41
                        or os.read(descriptor, 42) != (expected + '\n').encode('ascii')):
                    raise SourceExecutorError('source head mismatch')
            finally:
                os.close(descriptor)
        finally:
            os.close(git)
    finally:
        os.close(source)


def source_path(root, base, value, require_manifest):
    if not isinstance(value, str) or not value or '\0' in value or os.path.isabs(value):
        raise SourceExecutorError('source path unsupported')
    resolved = os.path.realpath(os.path.join(base, value))
    if os.path.commonpath((root, resolved)) != root:
        raise SourceExecutorError('source path unsupported')
    if require_manifest and (not os.path.isdir(resolved)
                             or not os.path.isfile(os.path.join(resolved, 'Cargo.toml'))):
        raise SourceExecutorError('source path unsupported')


def validate_manifest(root, path, value):
    if 'patch' in value or 'replace' in value:
        raise SourceExecutorError('manifest patch unsupported')
    base = os.path.dirname(path)
    sections = [value.get(name, {}) for name in ('dependencies', 'dev-dependencies', 'build-dependencies')]
    workspace = value.get('workspace', {})
    if not isinstance(workspace, dict):
        raise SourceExecutorError('invalid source model')
    sections.append(workspace.get('dependencies', {}))
    members = workspace.get('members', [])
    if not isinstance(members, list):
        raise SourceExecutorError('invalid source model')
    for member in members:
        source_path(root, base, member, False)
    package = value.get('package', {})
    if not isinstance(package, dict):
        raise SourceExecutorError('invalid source model')
    if 'workspace' in package:
        source_path(root, base, package['workspace'], True)
    for name in ('build', 'readme', 'license-file'):
        if isinstance(package.get(name), str):
            source_path(root, base, package[name], False)
    workspace_package = workspace.get('package', {})
    if not isinstance(workspace_package, dict):
        raise SourceExecutorError('invalid source model')
    for name in ('readme', 'license-file'):
        if isinstance(workspace_package.get(name), str):
            source_path(root, base, workspace_package[name], False)
    for target_kind in ('lib', 'bin', 'bench', 'test', 'example'):
        target_value = value.get(target_kind, [])
        target_list = [target_value] if isinstance(target_value, dict) else target_value
        if not isinstance(target_list, list) or not all(isinstance(item, dict) for item in target_list):
            raise SourceExecutorError('invalid source model')
        for item in target_list:
            if 'path' in item:
                source_path(root, base, item['path'], False)
    targets = value.get('target', {})
    if not isinstance(targets, dict):
        raise SourceExecutorError('invalid source model')
    for target in targets.values():
        if not isinstance(target, dict):
            raise SourceExecutorError('invalid source model')
        sections.extend(target.get(name, {}) for name in ('dependencies', 'dev-dependencies', 'build-dependencies'))
    for section in sections:
        if not isinstance(section, dict):
            raise SourceExecutorError('invalid source model')
        for spec in section.values():
            if isinstance(spec, str):
                continue
            if not isinstance(spec, dict):
                raise SourceExecutorError('invalid source model')
            if 'git' in spec or 'registry-index' in spec or spec.get('registry', 'crates-io') != 'crates-io':
                raise SourceExecutorError('dependency source unsupported')
            if 'path' in spec:
                source_path(root, base, spec['path'], True)


def validate_source_manifests(root, manifest):
    resolved = os.path.realpath(os.path.join(root, manifest))
    if os.path.commonpath((root, resolved)) != root or not os.path.isfile(resolved):
        raise SourceExecutorError('invalid manifest')
    def fail(error):
        raise SourceExecutorError('invalid source model') from error
    for current, directories, files in os.walk(root, topdown=True, onerror=fail, followlinks=False):
        if current == root and '.git' in directories:
            directories.remove('.git')
        if '.cargo' in directories:
            cargo_dir = os.path.join(current, '.cargo')
            if os.path.islink(cargo_dir) or any(os.path.lexists(os.path.join(cargo_dir, name))
                                                for name in ('config', 'config.toml')):
                raise SourceExecutorError('project cargo config unsupported')
        if 'Cargo.toml' in files:
            path = os.path.join(current, 'Cargo.toml')
            try:
                if os.stat(path).st_size > 4 * 1024 * 1024:
                    raise SourceExecutorError('invalid source model')
                with open(path, 'rb') as stream:
                    manifest_value = tomllib.load(stream)
            except (OSError, tomllib.TOMLDecodeError):
                raise SourceExecutorError('invalid source model') from None
            validate_manifest(root, path, manifest_value)


def validate_source_lock(root, workspace):
    if (not isinstance(workspace, str) or not os.path.isabs(workspace)
            or os.path.commonpath((root, workspace)) != root):
        raise SourceExecutorError('invalid workspace')
    lock_path = os.path.join(workspace, 'Cargo.lock')
    if not os.path.isfile(lock_path) or os.path.getsize(lock_path) > 16 * 1024 * 1024:
        raise SourceExecutorError('lockfile unsupported')
    try:
        with open(lock_path, 'rb') as stream:
            lock = tomllib.load(stream)
    except (OSError, tomllib.TOMLDecodeError):
        raise SourceExecutorError('lockfile unsupported') from None
    packages = lock.get('package')
    if not isinstance(packages, list) or not packages:
        raise SourceExecutorError('lockfile unsupported')
    for package in packages:
        if not isinstance(package, dict):
            raise SourceExecutorError('lockfile unsupported')
        registry = package.get('source')
        if registry is None:
            continue
        checksum = package.get('checksum')
        if (registry != 'registry+https://github.com/rust-lang/crates.io-index'
                or not isinstance(checksum, str) or not re.fullmatch(r'[a-f0-9]{64}', checksum)):
            raise SourceExecutorError('dependency source unsupported')


def _source_operation(manager, lifecycle, root, context, operation, package_index, deadline, baseline=None, workspace=None):
    if (os.geteuid() != 0 or type(context) is not SealedSourceContext
            or manager.root_bootstrap or not manager.deny_unix
            or not manager.bind_to or not os.path.isabs(root) or os.path.dirname(root) != '/run'
            or not re.fullmatch(r'velnor-publisher-[a-zA-Z0-9_-]+', os.path.basename(root))
            or operation not in ('locate', 'metadata', 'package', 'package-verify', 'semver')
            or (operation == 'locate' and not manager.deny_network)
            or (operation in ('locate', 'metadata') and package_index is not None)
            or (operation in ('package', 'package-verify', 'semver') and (type(package_index) is not int
                or not 0 <= package_index < len(context.packages)))):
        raise SourceExecutorError('invalid source operation')
    run_info = os.stat('/run', follow_symlinks=False)
    if not stat.S_ISDIR(run_info.st_mode) or run_info.st_uid != 0 or run_info.st_mode & 0o022:
        raise SourceExecutorError('invalid source custody')
    info = os.stat(root, follow_symlinks=False)
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
        raise SourceExecutorError('invalid source custody')
    if any(os.path.lexists('/.cargo/' + name) for name in ('config', 'config.toml')):
        raise SourceExecutorError('host cargo config unsupported')
    source = root + '/source'
    toolchain = root + '/rustup/toolchains/' + context.rust_version + '-x86_64-unknown-linux-gnu'
    toolbin = toolchain + '/bin'
    for directory in (source, root + '/rustup', root + '/rustup/toolchains', toolchain, toolbin):
        info = os.stat(directory, follow_symlinks=False)
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise SourceExecutorError('invalid source custody')
    verify_immutable_tree(source, deadline, True)
    verify_detached_head(source, context.source_sha)
    verify_source_inventory(source, context.source_files, deadline)
    validate_source_manifests(source, context.manifest)
    if operation != 'locate':
        validate_source_lock(source, workspace)
    verify_immutable_tree(toolchain, deadline, False)
    tools = {name: toolbin + '/' + name for name in ('cargo', 'rustc', 'rustdoc')}
    if 'semver' in dict(context.tools):
        for directory in (root + '/semver', root + '/git'):
            info = os.stat(directory, follow_symlinks=False)
            if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
                raise SourceExecutorError('invalid source custody')
            verify_immutable_tree(directory, deadline, directory == root + '/git')
        git_root = root + '/git'
        if (set(os.listdir(git_root)) != {'git', 'git-remote-http', 'git-remote-https'}
                or not os.path.islink(git_root + '/git-remote-https')
                or os.readlink(git_root + '/git-remote-https') != 'git-remote-http'):
            raise SourceExecutorError('invalid git bundle')
        tools['semver'] = root + '/semver/cargo-semver-checks'
        tools['git'] = git_root + '/git'
        tools['git_http'] = git_root + '/git-remote-http'
    for name, path in tools.items():
        verify_tool(path, dict(context.tools)[name], deadline)
    manifest = source + '/' + context.manifest
    search_path = toolbin + (':' + root + '/git' if 'git' in tools else '') + ':/usr/bin:/bin'
    env = {'PATH': search_path, 'HOME': '/tmp', 'CARGO_HOME': '/tmp/velnor-cargo-home',
           'RUSTC': tools['rustc'], 'RUSTDOC': tools['rustdoc'],
           'GIT_CONFIG_NOSYSTEM': '1', 'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_TERMINAL_PROMPT': '0',
           'GIT_CONFIG_COUNT': '1', 'GIT_CONFIG_KEY_0': 'safe.directory', 'GIT_CONFIG_VALUE_0': source}
    if operation == 'locate':
        program = tools['cargo']
        args = ['locate-project', '--workspace', '--offline', '--message-format', 'plain', '--manifest-path', manifest]
        cap = 16 * 1024
    elif operation == 'metadata':
        program = tools['cargo']
        args = ['metadata', '--locked', '--format-version', '1', '--no-deps', '--manifest-path', manifest]
        cap = 16 * 1024 * 1024
    elif operation == 'semver':
        if type(baseline) is not SemverBaseline or 'semver' not in tools or 'cc' not in dict(context.tools):
            raise SourceExecutorError('semver context unavailable')
        if os.path.lexists(toolbin + '/cc') or os.path.lexists(root + '/git/cc'):
            raise SourceExecutorError('host compiler shadowed')
        host_compiler = verify_host_compiler(dict(context.tools)['cc'], deadline)
        package = context.packages[package_index]
        if (baseline.source_sha != context.source_sha or baseline.package != package.name
                or baseline.target_version != package.version or baseline.manifest != package.manifest
                or baseline.features != package.features):
            raise SourceExecutorError('semver context mismatch')
        baseline_root = verify_semver_baseline(root, baseline, deadline)
        env['CARGO'] = tools['cargo']
        env['GIT_EXEC_PATH'] = root + '/git'
        env['CARGO_TARGET_DIR'] = '/tmp/velnor-target'
        program = tools['semver']
        args = ['check-release', '--manifest-path', source + '/' + package.manifest,
                '--package', package.name, '--baseline-root', baseline_root, '--default-features']
        if package.features:
            args.extend(['--features', ','.join(package.features)])
        cap = 1024 * 1024
    else:
        package = context.packages[package_index]
        if package.version is None:
            raise SourceExecutorError('package version unbound')
        program = '/usr/bin/python3'
        args = ['-I', '-S', '-c', PACKAGE_WRAPPER, tools['cargo'], manifest,
                package.name, package.version, ','.join(package.features),
                'true' if operation == 'package-verify' else 'false']
        cap = 64 * 1024 * 1024
    frame = json.dumps({'program': program, 'args': args, 'cwd': '/tmp', 'env': env, 'stdin': ''},
                       separators=(',', ':')).encode()
    if not 0 < len(frame) <= 1024 * 1024:
        raise SourceExecutorError('source frame too large')
    namespace = os.stat('/proc/self/ns/time').st_ino
    frame = struct.pack('>QQI', deadline, namespace, len(frame)) + frame
    output = os.memfd_create('velnor-source-output', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
    try:
        os.ftruncate(output, cap)
        fcntl.fcntl(output, fcntl.F_ADD_SEALS, fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK)
        lifecycle.supervise(manager, frame, deadline, output_fd=output)
        size = os.lseek(output, 0, os.SEEK_CUR)
        if not 0 <= size < cap or (operation != 'semver' and size == 0) or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
            raise SourceExecutorError('invalid source output')
        fcntl.fcntl(output, fcntl.F_ADD_SEALS, fcntl.F_SEAL_WRITE | fcntl.F_SEAL_SEAL)
        data = os.pread(output, size, 0)
        if len(data) != size or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
            raise SourceExecutorError('invalid source output')
        for name, path in tools.items():
            verify_tool(path, dict(context.tools)[name], deadline)
        verify_immutable_tree(source, deadline, True)
        verify_detached_head(source, context.source_sha)
        verify_source_inventory(source, context.source_files, deadline)
        verify_immutable_tree(toolchain, deadline, False)
        if operation == 'semver':
            verify_semver_baseline(root, baseline, deadline)
            if verify_host_compiler(dict(context.tools)['cc'], deadline) != host_compiler:
                raise SourceExecutorError('host compiler changed')
            return b''
        return data
    finally:
        os.close(output)


class SourceSession:
    def __init__(self, manager, locator, lifecycle, root, initial, deadline, view_identity):
        if (type(deadline) is not int or not 0 < deadline < 2**64
                or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline
                or type(initial) is not SealedSourceContext
                or any(package.version is not None or package.manifest is not None for package in initial.packages)):
            raise SourceExecutorError('invalid source context')
        if (not isinstance(view_identity, tuple) or len(view_identity) != 2
                or not all(type(value) is int and value >= 0 for value in view_identity)):
            raise SourceExecutorError('source view identity invalid')
        self.view_identity = view_identity
        self.root = root
        self.verify_view()
        self.manager = manager
        self.locator = locator
        self.workspace = None
        self.lifecycle = lifecycle
        self.initial = initial
        self._deadline = deadline
        self.metadata_bytes = None
        self.bound = None
        self.poisoned = False

    def verify_view(self):
        info = os.stat(self.root, follow_symlinks=False)
        if (not stat.S_ISDIR(info.st_mode) or info.st_uid != 0
                or stat.S_IMODE(info.st_mode) != 0o711
                or (info.st_dev, info.st_ino) != self.view_identity):
            raise SourceExecutorError('source view changed')

    def metadata(self):
        if self.poisoned or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= self._deadline:
            raise SourceExecutorError('source session deadline')
        self.verify_view()
        source = self.root + '/source'
        location = _source_operation(self.locator, self.lifecycle, self.root, self.initial, 'locate', None, self._deadline)
        try:
            location = location.decode('utf-8').strip()
        except UnicodeError:
            raise SourceExecutorError('invalid workspace') from None
        if not os.path.isabs(location) or '\n' in location or '\0' in location:
            raise SourceExecutorError('invalid workspace')
        workspace_manifest = os.path.realpath(location)
        if (os.path.commonpath((source, workspace_manifest)) != source
                or os.path.basename(workspace_manifest) != 'Cargo.toml'
                or os.path.relpath(workspace_manifest, source) not in {item.path for item in self.initial.source_files}):
            raise SourceExecutorError('invalid workspace')
        workspace = os.path.dirname(workspace_manifest)
        if self.workspace is not None and self.workspace != workspace:
            raise SourceExecutorError('workspace changed')
        validate_source_lock(source, workspace)
        data = _source_operation(self.manager, self.lifecycle, self.root, self.initial, 'metadata', None, self._deadline, workspace=workspace)
        self.verify_view()
        if not isinstance(data, bytes) or not 0 < len(data) < 16 * 1024 * 1024:
            raise SourceExecutorError('invalid workspace metadata')
        if self.metadata_bytes is not None:
            if data != self.metadata_bytes:
                raise SourceExecutorError('workspace metadata changed')
            return data
        try:
            value = json.loads(data, object_pairs_hook=unique_object)
        except (ValueError, UnicodeError):
            raise SourceExecutorError('invalid workspace metadata') from None
        if not isinstance(value, dict) or not isinstance(value.get('packages'), list) or not isinstance(value.get('workspace_members'), list):
            raise SourceExecutorError('invalid workspace metadata')
        source = self.root + '/source'
        workspace = value.get('workspace_root')
        if not isinstance(workspace, str) or not os.path.isabs(workspace):
            raise SourceExecutorError('invalid workspace metadata')
        workspace = os.path.realpath(workspace)
        if os.path.commonpath((source, workspace)) != source:
            raise SourceExecutorError('workspace outside source')
        if workspace != os.path.dirname(workspace_manifest):
            raise SourceExecutorError('workspace changed')
        workspace_manifest = os.path.relpath(os.path.join(workspace, 'Cargo.toml'), source)
        if workspace_manifest not in {entry.path for entry in self.initial.source_files}:
            raise SourceExecutorError('workspace manifest untracked')
        members = value['workspace_members']
        if not all(isinstance(item, str) for item in members) or len(set(members)) != len(members):
            raise SourceExecutorError('invalid workspace metadata')
        packages = {}
        seen_ids = set()
        tracked = {entry.path for entry in self.initial.source_files}
        for item in value['packages']:
            if not isinstance(item, dict) or not isinstance(item.get('id'), str):
                raise SourceExecutorError('invalid workspace metadata')
            if item['id'] in seen_ids:
                raise SourceExecutorError('invalid workspace metadata')
            seen_ids.add(item['id'])
            if item['id'] not in members:
                continue
            name, version, manifest = item.get('name'), item.get('version'), item.get('manifest_path')
            if (not isinstance(name, str) or name in packages or not isinstance(version, str)
                    or not isinstance(manifest, str) or not os.path.isabs(manifest)):
                raise SourceExecutorError('invalid workspace metadata')
            resolved = os.path.realpath(manifest)
            if os.path.commonpath((source, resolved)) != source:
                raise SourceExecutorError('workspace manifest outside source')
            relative = os.path.relpath(resolved, source)
            if relative not in tracked:
                raise SourceExecutorError('workspace manifest untracked')
            packages[name] = (version, relative)
        if not set(members).issubset(seen_ids) or any(package.name not in packages for package in self.initial.packages):
            raise SourceExecutorError('workspace package missing')
        self.workspace = workspace
        self.metadata_bytes = data
        self.metadata_packages = packages
        return data

    def bind(self, bound):
        if (self.poisoned or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= self._deadline
                or self.metadata_bytes is None or self.bound is not None or type(bound) is not SealedSourceContext):
            raise SourceExecutorError('package context unavailable')
        self.verify_view()
        first = self.initial
        if (bound.identity != first.identity or bound.source_sha != first.source_sha or bound.rust_version != first.rust_version or bound.manifest != first.manifest or bound.authentication != first.authentication
                or bound.source_files != first.source_files or bound.tools != first.tools
                or len(bound.packages) != len(first.packages)):
            raise SourceExecutorError('package context changed')
        for previous, current in zip(first.packages, bound.packages, strict=True):
            if (previous.name != current.name or previous.features != current.features
                    or previous.upload_config_sha256 != current.upload_config_sha256
                    or current.version is None or current.manifest is None
                    or self.metadata_packages.get(current.name) != (current.version, current.manifest)):
                raise SourceExecutorError('package context changed')
        if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= self._deadline:
            raise SourceExecutorError('package context deadline')
        self.verify_view()
        self.bound = bound

    def package(self, index, verify):
        if (self.poisoned or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= self._deadline
                or self.bound is None or type(verify) is not bool):
            raise SourceExecutorError('package context unavailable')
        self.verify_view()
        operation = 'package-verify' if verify else 'package'
        result = _source_operation(self.manager, self.lifecycle, self.root, self.bound, operation, index, self._deadline, workspace=self.workspace)
        self.verify_view()
        return result

    def semver(self, index, baseline):
        if (self.poisoned or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= self._deadline
                or self.bound is None):
            raise SourceExecutorError('semver context unavailable')
        self.verify_view()
        self.poisoned = True
        result = _source_operation(self.manager, self.lifecycle, self.root, self.bound,
                                   'semver', index, self._deadline, baseline, self.workspace)
        self.verify_view()
        self.poisoned = False
        return result
