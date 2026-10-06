"""Descriptor-relative, atomic installation of exact qualified Mise bytes."""
import hashlib
import json
import os
import platform
import stat


def owned_directory(parent, name, create=False):
    if create:
        try:
            os.mkdir(name, 0o700, dir_fd=parent)
        except FileExistsError:
            pass
    descriptor = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
    info = os.fstat(descriptor)
    if info.st_uid != os.geteuid() or info.st_mode & 0o022:
        os.close(descriptor)
        raise ValueError('mise_bootstrap_directory_owner')
    return descriptor


def root_directory(domain):
    path = os.environ['RUNNER_TEMP']
    parts = path.split('/')
    if (not path.startswith('/') or any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in path)
            or any(part in ('', '.', '..') for part in parts[1:])):
        raise ValueError('mise_bootstrap_temp_path')
    descriptor = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in parts[1:]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        info = os.fstat(descriptor)
        if info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ValueError('mise_bootstrap_temp_owner')
        root = path + '/' + DOMAINS[domain]
        if os.environ.get('MISE_DATA_DIR') != root:
            raise ValueError('mise_bootstrap_root_binding')
        for part in DOMAINS[domain].split('/'):
            child = owned_directory(descriptor, part, create=True)
            os.close(descriptor)
            descriptor = child
        return descriptor, root
    except BaseException:
        os.close(descriptor)
        raise


def verified_binary(parent, digest):
    try:
        descriptor = os.open('mise', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    except FileNotFoundError:
        return False
    try:
        info = os.fstat(descriptor)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid()
                or info.st_nlink != 1 or info.st_size > executable_limit()):
            raise ValueError('mise_bootstrap_binary_shape')
        if info.st_mode & 0o022 or not info.st_mode & 0o100:
            return False
        with os.fdopen(os.dup(descriptor), 'rb') as stream:
            return stream_sha256(stream) == digest
    finally:
        os.close(descriptor)


def atomic_file(parent, name, payload, mode):
    temporary = '.velnor-acquisition-' + os.urandom(16).hex()
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                         mode, dir_fd=parent)
    try:
        remaining = memoryview(payload)
        while remaining:
            count = os.write(descriptor, remaining)
            if count <= 0:
                raise ValueError('mise_bootstrap_short_write')
            remaining = remaining[count:]
        os.fchmod(descriptor, mode)
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = None
        os.replace(temporary, name, src_dir_fd=parent, dst_dir_fd=parent)
        os.fsync(parent)
    finally:
        if descriptor is not None:
            os.close(descriptor)
        try:
            os.unlink(temporary, dir_fd=parent)
        except FileNotFoundError:
            pass


def acquire(domain, configuration):
    if OUTPUT_PURPOSE not in ('workflow', 'source-intent'):
        raise ValueError('mise_bootstrap_output_purpose')
    if (platform.system(), platform.machine()) != tuple(configuration['platform']):
        raise ValueError('mise_bootstrap_platform')
    root, path = root_directory(domain)
    try:
        binary_root = owned_directory(root, 'bin', create=True)
        try:
            if not verified_binary(binary_root, configuration['binary_sha256']):
                archive = download_asset(configuration['asset_url'], configuration['archive_sha256'])
                if configuration['format'] == 'tar-gzip':
                    binary = extract_binary(archive, configuration['binary_sha256'],
                                            configuration['member'])
                elif configuration['format'] == 'binary' and not configuration['member']:
                    binary = archive
                else:
                    raise ValueError('mise_bootstrap_format')
                if hashlib.sha256(binary).hexdigest() != configuration['binary_sha256']:
                    raise ValueError('mise_bootstrap_binary_digest')
                atomic_file(binary_root, 'mise', binary, 0o700)
            if not verified_binary(binary_root, configuration['binary_sha256']):
                raise ValueError('mise_bootstrap_final_digest')
            receipt = json.dumps(configuration, sort_keys=True, separators=(',', ':')).encode()
            atomic_file(root, '.velnor-mise-receipt.json', receipt, 0o600)
        finally:
            os.close(binary_root)
        if OUTPUT_PURPOSE == 'workflow':
            with open(os.environ['GITHUB_PATH'], 'a', encoding='utf-8') as output:
                output.write(path + '/bin\n')
            with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
                output.write('verified=true\n')
    finally:
        os.close(root)
