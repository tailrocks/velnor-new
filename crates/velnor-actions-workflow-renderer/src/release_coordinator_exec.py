# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
import re
import stat
import struct
import sys
import time

CONTEXT = frozenset((
    'GITHUB_REPOSITORY', 'GITHUB_REPOSITORY_ID', 'GITHUB_REPOSITORY_OWNER_ID',
    'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT', 'GITHUB_JOB', 'GITHUB_SHA',
    'GITHUB_REF', 'GITHUB_EVENT_NAME', 'GITHUB_WORKFLOW_REF', 'GITHUB_WORKFLOW_SHA',
    'GITHUB_SERVER_URL', 'GITHUB_API_URL', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'RUNNER_ARCH',
    'VELNOR_PREFLIGHT_ELIGIBLE', 'VELNOR_PREFLIGHT_SOURCE_SHA',
    'VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID', 'VELNOR_PREFLIGHT_RUST_ARTIFACT_ID',
    'VELNOR_GENERATOR_VERSION', 'VELNOR_GENERATOR_SOURCE_SHA', 'VELNOR_GENERATOR_MANIFEST_SHA256',
    'GIT_TOKEN',
))


def read_exact(size):
    result = bytearray()
    while len(result) < size:
        part = os.read(0, size - len(result))
        if not part:
            raise ValueError('short coordinator frame')
        result.extend(part)
    return result


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate field')
        result[key] = value
    return result


def open_binary(version, digest):
    parent = os.path.dirname(os.path.abspath(__file__))
    if (os.path.dirname(parent) != '/run'
            or not re.fullmatch(r'velnor-publisher-[a-zA-Z0-9_-]+', os.path.basename(parent))
            or not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version)
            or not re.fullmatch(r'[a-f0-9]{64}', digest)):
        raise ValueError('invalid coordinator identity')
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    directory = os.open('/run', flags)
    try:
        for component in (os.path.basename(parent), 'velnor', 'bin'):
            current = os.fstat(directory)
            if current.st_uid != 0 or current.st_mode & 0o022:
                raise ValueError('invalid coordinator custody')
            next_directory = os.open(component, flags, dir_fd=directory)
            os.close(directory)
            directory = next_directory
        current = os.fstat(directory)
        if current.st_uid != 0 or current.st_mode & 0o022:
            raise ValueError('invalid coordinator custody')
        binary = os.open('velnor-actions-' + version, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC,
                         dir_fd=directory)
    finally:
        os.close(directory)
    try:
        info = os.fstat(binary)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                or info.st_mode & 0o077 or not info.st_mode & 0o100
                or not 0 < info.st_size <= 100 * 1024 * 1024):
            raise ValueError('invalid coordinator binary')
        hasher = hashlib.sha256()
        while part := os.read(binary, 1024 * 1024):
            hasher.update(part)
        if hasher.hexdigest() != digest:
            raise ValueError('coordinator hash mismatch')
        os.lseek(binary, 0, os.SEEK_SET)
        return binary
    except BaseException:
        os.close(binary)
        raise


def main():
    if os.geteuid() != 0:
        raise ValueError('invalid coordinator identity')
    deadline, namespace, size = struct.unpack('>QQI', read_exact(20))
    if (deadline <= time.clock_gettime_ns(time.CLOCK_BOOTTIME)
            or namespace != os.stat('/proc/self/ns/time').st_ino or not 0 < size <= 1024 * 1024):
        raise ValueError('invalid coordinator frame')
    frame = json.loads(read_exact(size), object_pairs_hook=unique_object)
    if not isinstance(frame, dict) or set(frame) != {'version', 'sha256', 'env'}:
        raise ValueError('invalid coordinator frame')
    version, digest, env = frame['version'], frame['sha256'], frame['env']
    if (not isinstance(version, str) or not isinstance(digest, str) or not isinstance(env, dict)
            or not set(env).issubset(CONTEXT)
            or not all(isinstance(value, str) and '\0' not in value for value in env.values())):
        raise ValueError('invalid coordinator frame')
    binary = open_binary(version, digest)
    root = os.path.dirname(os.path.abspath(__file__))
    env.update({'PATH': '/usr/bin:/bin', 'HOME': root, 'TMPDIR': root, 'RUNNER_TEMP': root,
                'VELNOR_OPERATION_DEADLINE_BOOTTIME_NS': str(deadline)})
    for name in os.listdir('/proc/self/fd'):
        if name.isdigit() and int(name) > 2 and int(name) != binary:
            try:
                os.close(int(name))
            except OSError:
                pass
    if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
        raise ValueError('coordinator deadline')
    os.execve(binary, ['velnor-actions', 'crate-release', 'publish-supervise'], env)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, UnicodeError, json.JSONDecodeError):
        raise SystemExit('coordinator launch rejected') from None
