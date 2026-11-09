# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import hashlib
import os
import pathlib
import stat
import tempfile


def admit_file(fd, mode=None):
    item = os.fstat(fd)
    if not stat.S_ISREG(item.st_mode) or item.st_uid != os.getuid() or item.st_mode & 0o022:
        raise RuntimeError('invalid premerge source file')
    if mode is not None and stat.S_IMODE(item.st_mode) != mode:
        raise RuntimeError('invalid premerge source mode')
    return item


def admit_directory(path, private):
    if not path.is_absolute() or path.resolve(strict=True) != path or any(ord(c) < 32 for c in str(path)):
        raise RuntimeError('invalid premerge source path')
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    item = os.fstat(fd)
    if item.st_uid != os.getuid() or item.st_mode & 0o022 or (private and stat.S_IMODE(item.st_mode) != 0o700):
        os.close(fd)
        raise RuntimeError('invalid premerge source directory')
    return fd


def source_directory(first):
    runner = pathlib.Path(os.environ['RUNNER_TEMP'])
    runner_fd = admit_directory(runner, False)
    os.close(runner_fd)
    if first:
        path = pathlib.Path(tempfile.mkdtemp(prefix='velnor-premerge-', dir=runner))
        env = pathlib.Path(os.environ['GITHUB_ENV'])
        if not env.is_absolute() or env.resolve(strict=True) != env or not env.is_relative_to(runner):
            raise RuntimeError('invalid premerge environment file')
        fd = os.open(env, os.O_WRONLY | os.O_APPEND | os.O_NOFOLLOW | os.O_NONBLOCK)
        try:
            admit_file(fd)
            data = ('VELNOR_PREMERGE_SOURCE=' + str(path) + '\n').encode()
            if os.write(fd, data) != len(data):
                raise RuntimeError('incomplete premerge environment write')
        finally:
            os.close(fd)
    else:
        path = pathlib.Path(os.environ['VELNOR_PREMERGE_SOURCE'])
        if path.parent != runner or not path.name.startswith('velnor-premerge-'):
            raise RuntimeError('invalid premerge source directory')
    return admit_directory(path, True)


def write_chunk(index, encoded):
    directory = source_directory(index == 0)
    try:
        data = base64.b64decode(encoded, validate=True)
        if not data or len(data) > 9000:
            raise RuntimeError('invalid premerge chunk')
        fd = os.open('chunk-' + str(index), os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=directory)
        try:
            admit_file(fd, 0o600)
            if os.write(fd, data) != len(data):
                raise RuntimeError('incomplete premerge source write')
            os.fchmod(fd, 0o400)
        finally:
            os.close(fd)
    finally:
        os.close(directory)


def execute_source(count, expected):
    directory = source_directory(False)
    try:
        if set(os.listdir(directory)) != {'chunk-' + str(index) for index in range(count)}:
            raise RuntimeError('invalid premerge source inventory')
        data = bytearray()
        for index in range(count):
            fd = os.open('chunk-' + str(index), os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=directory)
            try:
                item = admit_file(fd, 0o400)
                if not 0 < item.st_size <= 9000:
                    raise RuntimeError('invalid premerge source size')
                with os.fdopen(fd, 'rb', closefd=False) as stream:
                    chunk = stream.read(9001)
                if len(chunk) != item.st_size:
                    raise RuntimeError('premerge source changed')
                data.extend(chunk)
            finally:
                os.close(fd)
        if len(data) > 1024 * 1024 or hashlib.sha256(data).hexdigest() != expected:
            raise RuntimeError('premerge source digest mismatch')
    finally:
        os.close(directory)
    exec(compile(bytes(data), '<premerge-producer>', 'exec'), {'__name__': '__main__'})
