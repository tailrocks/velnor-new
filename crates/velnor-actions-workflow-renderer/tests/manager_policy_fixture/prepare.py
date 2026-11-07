# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

from pathlib import Path
from compression import zstd
import argparse
import hashlib
import io
import json
import os
import re
import signal
import stat
import subprocess
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request


def digest(data):
    return hashlib.sha256(data).hexdigest()


def deb_data(data):
    if data[:8] != b'!<arch>\n':
        raise ValueError('invalid package container')
    position = 8
    members = {}
    while position < len(data):
        header = data[position:position + 60]
        if len(header) != 60 or header[-2:] != b'`\n':
            raise ValueError('invalid package member')
        size = int(header[48:58].strip())
        name = header[:16].decode('ascii').strip().removesuffix('/')
        position += 60
        if name in members or size < 0 or position + size > len(data):
            raise ValueError('invalid package member extent')
        members[name] = data[position:position + size]
        position += size + size % 2
    if position != len(data) or members.get('debian-binary') != b'2.0\n':
        raise ValueError('invalid package container extent')
    with zstd.ZstdFile(io.BytesIO(members['data.tar.zst'])) as stream:
        payload = stream.read(256 * 1024 * 1024 + 1)
    if len(payload) > 256 * 1024 * 1024:
        raise ValueError('package payload exceeds bound')
    return payload


class OfficialRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, url):
        official_url(url)
        return super().redirect_request(request, fp, code, message, headers, url)


def official_url(url):
    value = urllib.parse.urlsplit(url)
    if (value.scheme != 'https' or value.netloc != 'archive.ubuntu.com'
            or not value.path.startswith('/ubuntu/pool/main/')
            or value.query or value.fragment):
        raise ValueError('unapproved package origin')


def archive_bytes(url, record, cache, offline, deadline):
    official_url(url)
    expected = record['sha256']
    if not re.fullmatch('[0-9a-f]{64}', expected) or not 0 < record['size'] <= 64 * 1024 * 1024:
        raise ValueError('invalid package identity')
    path = cache / expected
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    except FileNotFoundError:
        descriptor = None
    if descriptor is not None:
        with os.fdopen(descriptor, 'rb') as stream:
            metadata = os.fstat(stream.fileno())
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_size != record['size']:
                raise ValueError('invalid cached package')
            data = stream.read(record['size'] + 1)
    else:
        if offline:
            raise ValueError('package absent from offline cache')
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ValueError('fixture preparation deadline')
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), OfficialRedirect())
        with opener.open(url, timeout=min(30, remaining)) as response:
            official_url(response.url)
            data = response.read(record['size'] + 1)
    if len(data) != record['size'] or digest(data) != expected:
        raise ValueError('package checksum mismatch')
    if time.monotonic() >= deadline:
        raise ValueError('fixture preparation deadline')
    if descriptor is None:
        with tempfile.NamedTemporaryFile(dir=cache, delete=False) as temporary:
            temporary.write(data)
            temporary.flush()
            os.fchmod(temporary.fileno(), 0o400)
            temporary_path = Path(temporary.name)
        try:
            os.link(temporary_path, path)
        except FileExistsError:
            archive_bytes(url, record, cache, True, deadline)
        finally:
            temporary_path.unlink()
    return data


def write_file(root, path, data, mode):
    if (not path.startswith('/usr/lib/') and path != '/etc/systemd/system.conf'
            or '..' in Path(path).parts or mode not in (0o644, 0o755)):
        raise ValueError('invalid fixture file path')
    destination = root / path.lstrip('/')
    destination.parent.mkdir(parents=True, exist_ok=True)
    with destination.open('xb') as stream:
        stream.write(data)
    destination.chmod(mode)


def prepare(materials, cache, offline, output, deadline):
    if (materials['schema'] != 1 or materials['role'] != 'manager-policy-check-only'
            or not 0 < len(materials['archives']) <= 32):
        raise ValueError('invalid fixture materials')
    context = Path(tempfile.mkdtemp(prefix='manager-policy-', dir=output))
    root = context / 'rootfs'
    root.mkdir()
    files = {}
    for url, record in materials['archives'].items():
        blob = archive_bytes(url, record, cache, offline, deadline)
        with tarfile.open(fileobj=io.BytesIO(deb_data(blob))) as archive:
            for path, expected in record['files'].items():
                if path in files:
                    raise ValueError('conflicting fixture files')
                member = archive.getmember('.' + path)
                if not member.isfile() or member.size != expected['size']:
                    raise ValueError('invalid fixture payload member')
                data = archive.extractfile(member).read()
                if digest(data) != expected['sha256']:
                    raise ValueError('fixture payload checksum mismatch')
                write_file(root, path, data, expected['mode'])
                files[path] = expected
    if len(files) != 21:
        raise ValueError('incomplete fixture code closure')
    for path, record in materials['global_configuration'].items():
        data = record['content'].encode()
        if digest(data) != record['sha256']:
            raise ValueError('fixture global policy checksum mismatch')
        write_file(root, path, data, record['mode'])
    unit = ('[Unit]\nAfter=dbus.service\nRequires=dbus.service\nBefore=multi-user.target\n'
            '[Service]\nType=oneshot\nRemainAfterExit=yes\n'
            'ExecStart=/usr/bin/systemctl set-environment LANG=C.UTF-8 PATH=/usr/bin:/bin SGX_AESM_ADDR=fixture\n'
            '[Install]\nWantedBy=multi-user.target\n')
    write_file(root, '/usr/lib/systemd/system/fixture-manager-environment.service', unit.encode(), 0o644)
    link = root / 'etc/systemd/system/multi-user.target.wants/fixture-manager-environment.service'
    link.parent.mkdir(parents=True, exist_ok=True)
    link.symlink_to('/usr/lib/systemd/system/fixture-manager-environment.service')
    for directory, _, _ in os.walk(root):
        Path(directory).chmod(0o755)
    template = Path(__file__).resolve().parent
    for name in ('Dockerfile', 'verify-files.py', '.dockerignore', 'global-policy.json', 'proof.json'):
        (context / name).write_bytes((template / name).read_bytes())
    return context


def preparation_timeout(_signal, _frame):
    raise TimeoutError('fixture preparation deadline')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    args.cache.mkdir(mode=0o700, parents=True, exist_ok=True)
    args.output.mkdir(mode=0o700, parents=True, exist_ok=True)
    materials = json.loads(Path(__file__).with_name('materials.json').read_text())
    deadline = time.monotonic() + 300
    signal.signal(signal.SIGALRM, preparation_timeout)
    signal.setitimer(signal.ITIMER_REAL, 300)
    try:
        context = prepare(materials, args.cache, args.offline, args.output, deadline)
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
    print(context)


if __name__ == '__main__':
    main()
