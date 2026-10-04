"""Fixed APT adapter primitives; configuration never supplies commands."""
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import stat
import subprocess
import struct
import tarfile


def require(condition, message):
    if not condition:
        raise ValueError(message)


def pairs(items):
    result = {}
    for key, value in items:
        require(key not in result, 'duplicate JSON field: ' + key)
        result[key] = value
    return result


def loads(text):
    def invalid(value):
        raise ValueError('invalid JSON number: ' + value)
    return json.loads(text, object_pairs_hook=pairs, parse_constant=invalid)


def regular(path):
    path = Path(path)
    for parent in [path, *path.parents]:
        require(not parent.is_symlink(), 'symlink forbidden: ' + str(parent))
    require(stat.S_ISREG(path.lstat().st_mode), 'regular file required: ' + str(path))
    return path


def read_bytes(path):
    path = regular(path)
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(descriptor, 'rb') as stream:
        require(stat.S_ISREG(os.fstat(stream.fileno()).st_mode), 'regular file required')
        return stream.read()


def read_json(path):
    return loads(read_bytes(path).decode('utf-8'))


def digest(path):
    return hashlib.sha256(read_bytes(path)).hexdigest()


def config_digest(config):
    return hashlib.sha256(json.dumps(config, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def write_json(path, document):
    path = Path(path)
    require(not path.exists() and not path.is_symlink(), 'output already exists: ' + str(path))
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'w') as stream:
        json.dump(document, stream, sort_keys=True, separators=(',', ':'))
        stream.write('\n')


def run(argv):
    require(isinstance(argv, list) and all(isinstance(x, str) for x in argv), 'argv list required')
    return subprocess.run(argv, check=True, stdout=subprocess.PIPE, text=True).stdout


def deb_payload(path):
    deb = regular(path)
    payload = subprocess.run(['dpkg-deb', '--fsys-tarfile', str(deb)], check=True,
                             stdout=subprocess.PIPE).stdout
    files = {}
    with tarfile.open(fileobj=io.BytesIO(payload), mode='r:') as archive:
        seen = set()
        for member in archive:
            path = PurePosixPath(member.name)
            require(not path.is_absolute() and '..' not in path.parts, 'unsafe deb archive path')
            require(member.isdir() or member.isfile(), 'deb links and special files forbidden')
            canonical = str(path)
            require(canonical not in seen, 'duplicate deb archive path')
            seen.add(canonical)
            if member.isfile():
                stream = archive.extractfile(member)
                require(stream is not None, 'unreadable deb archive member')
                files[canonical] = stream.read()
    return files



def elf_identity(binary, arch):
    require(arch in ('amd64', 'arm64'), 'unknown ELF architecture')
    require(len(binary) >= 64 and binary[:7] == b'\x7fELF\x02\x01\x01',
            'binary must be ELF64 little-endian version 1')
    kind, machine, version = struct.unpack_from('<HHI', binary, 16)
    require(kind in (2, 3) and version == 1, 'ELF must be executable or shared object version 1')
    require(machine == {'amd64': 62, 'arm64': 183}[arch], 'ELF architecture mismatch')
    require(struct.unpack_from('<H', binary, 52)[0] == 64, 'invalid ELF64 header size')
