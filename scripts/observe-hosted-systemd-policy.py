# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import stat
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'crates/velnor-actions-workflow-renderer/src'


def identity(path):
    value = os.stat(path)
    return {'device': value.st_dev, 'inode': value.st_ino}


def digest_file(path, limit, deadline):
    descriptor = os.open(path, os.O_RDONLY | os.O_CLOEXEC)
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or not 0 <= before.st_size <= limit:
            raise ValueError('policy observation file invalid')
        digest = hashlib.sha256()
        total = 0
        while data := os.read(descriptor, 65536):
            if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
                raise ValueError('policy observation deadline')
            total += len(data)
            if total > limit:
                raise ValueError('policy observation file oversized')
            digest.update(data)
        after = os.fstat(descriptor)
        shape = lambda item: (item.st_dev, item.st_ino, item.st_size, item.st_mtime_ns, item.st_ctime_ns)
        if shape(before) != shape(after) or total != before.st_size:
            raise ValueError('policy observation file changed')
        return {'sha256': digest.hexdigest(), 'size': total, 'uid': before.st_uid,
                'mode': stat.S_IMODE(before.st_mode), 'device': before.st_dev, 'inode': before.st_ino}
    finally:
        os.close(descriptor)


def process(pid, deadline):
    base = Path('/proc') / str(pid)
    with (base / 'stat').open() as stream:
        start = stream.read(4096).rsplit(')', 1)[1].split()[19]
    return {'pid': pid, 'start': start, 'root': identity(base / 'root'),
            'namespaces': {name: identity(base / 'ns' / name) for name in ('pid', 'mnt', 'time')},
            'executable': digest_file(base / 'exe', 256 * 1024 * 1024, deadline)}


def policy_files(roots, unit, deadline):
    scopes = (unit, 'velnor-release-.service', 'velnor-.service', 'service')
    result = {}
    for root in roots:
        if type(root) is not str or not root.startswith('/') or '..' in Path(root).parts:
            raise ValueError('invalid observed UnitPath')
        for scope in scopes:
            for suffix in ('', '.d', '.wants', '.requires', '.upholds'):
                path = Path(root) / (scope + suffix)
                if not path.exists() and not path.is_symlink():
                    continue
                if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
                    raise ValueError('policy observation deadline')
                entries = [path]
                if path.is_dir() and not path.is_symlink():
                    entries += sorted(path.iterdir())
                for entry in entries:
                    if len(result) >= 4096:
                        raise ValueError('policy observation entry limit')
                    info = entry.lstat()
                    value = {'uid': info.st_uid, 'mode': stat.S_IMODE(info.st_mode),
                             'device': info.st_dev, 'inode': info.st_ino}
                    if stat.S_ISLNK(info.st_mode):
                        value.update({'kind': 'symlink', 'target': os.readlink(entry)})
                    elif stat.S_ISREG(info.st_mode):
                        value.update({'kind': 'file', **digest_file(entry, 1024 * 1024, deadline)})
                    elif stat.S_ISDIR(info.st_mode):
                        value['kind'] = 'directory'
                    else:
                        value['kind'] = 'special'
                    result[str(entry)] = value
    return result


def capture():
    sys.path.insert(0, str(SOURCE))
    import release_manager_bus as bus
    deadline = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 10_000_000_000
    root = Path(tempfile.mkdtemp(prefix='velnor-publisher-', dir='/run'))
    root.chmod(0o700)
    manager = None
    try:
        hashes = []
        for name in ('release_launcher_watch.py', 'release_supervisor_claim.py'):
            data = (SOURCE / name).read_bytes()
            path = root / name
            path.write_bytes(data)
            path.chmod(0o500)
            hashes.append(hashlib.sha256(data).hexdigest())
        unit = 'velnor-release-' + secrets.token_hex(16) + '.service'
        manager = bus.Manager.watch_manager(str(root / 'release_launcher_watch.py'),
                    str(root / 'release_supervisor_claim.py'), 10, unit, *hashes)
        owner = manager.subscribe(deadline)
        pid = manager._method('org.freedesktop.DBus', '/org/freedesktop/DBus',
                    'org.freedesktop.DBus', 'GetConnectionUnixProcessID', [('s', owner)], deadline, 'u')
        uid = manager._method('org.freedesktop.DBus', '/org/freedesktop/DBus',
                    'org.freedesktop.DBus', 'GetConnectionUnixUser', [('s', owner)], deadline, 'u')
        if uid != 0 or pid <= 0:
            raise ValueError('systemd owner observation invalid')
        path = '/org/freedesktop/systemd1'
        interface = 'org.freedesktop.systemd1.Manager'
        version = manager._get(path, interface, 'Version', 's', deadline)
        roots = manager._get(path, interface, 'UnitPath', 'as', deadline)
        daemon = process(pid, deadline)
        controller_root = identity('/proc/self/root')
        controller_namespaces = {name: identity('/proc/self/ns/' + name) for name in ('pid', 'mnt', 'time')}
        if daemon['root'] != controller_root or daemon['namespaces'] != controller_namespaces:
            raise ValueError('manager and controller views differ')
        files = policy_files(roots, unit, deadline)
        if (manager._owner(deadline) != owner or process(pid, deadline) != daemon
                or manager._get(path, interface, 'UnitPath', 'as', deadline) != roots
                or policy_files(roots, unit, deadline) != files):
            raise ValueError('systemd observation changed')
        return {'schema': 1, 'role': 'observation-only', 'architecture': os.uname().machine,
                'owner': owner, 'owner_uid': uid, 'daemon': daemon,
                'controller_root': controller_root, 'controller_namespaces': controller_namespaces,
                'version': version, 'unit_path': roots, 'policy_files': files}
    finally:
        if manager is not None:
            manager.close()
        shutil.rmtree(root)


def main():
    if sys.argv[1:] == ['--capture']:
        if os.geteuid() != 0:
            raise ValueError('root observation required')
        print(json.dumps(capture(), sort_keys=True, separators=(',', ':')))
        return
    diagnostic = sys.argv[1:] == ['--diagnostic-report']
    if sys.argv[1:] not in ([], ['--diagnostic-report']) or os.uname().machine != 'x86_64':
        raise ValueError('hosted observation requires Linux x86_64')
    release = Path('/etc/os-release').read_text().splitlines()
    if 'ID=ubuntu' not in release or 'VERSION_ID="26.04"' not in release:
        raise ValueError('hosted observation requires Ubuntu26.04')
    result = subprocess.run(['/usr/bin/sudo', '-n', '/usr/bin/env', '-i',
                '/usr/bin/python3', '-I', '-S', '-B', str(Path(__file__).resolve()), '--capture'],
                check=True, capture_output=True, timeout=20)
    if not 0 < len(result.stdout) <= 2 * 1024 * 1024:
        raise ValueError('systemd observation output limit')
    report = json.loads(result.stdout)
    if report.get('role') != 'observation-only' or report.get('architecture') != 'x86_64':
        raise ValueError('systemd observation role mismatch')
    if diagnostic:
        print(result.stdout.decode(), file=sys.stderr, end='')
        raise SystemExit('systemd policy observed; admission remains unproven')
    (ROOT / 'hosted-systemd-policy-observation.json').write_bytes(result.stdout)
    print(result.stdout.decode(), end='')
    evidence = {'schema': 1, 'source': 'mise-task-v1', 'head': os.environ['VELNOR_CHECK_HEAD'],
                'check_id': os.environ['VELNOR_CHECK_ID'], 'platform': os.environ['VELNOR_CHECK_PLATFORM'],
                'scenarios': [{'id': 'systemd-policy-observed', 'executed': True, 'status': 'passed'}]}
    (ROOT / 'hosted-systemd-policy-evidence.json').write_text(json.dumps(evidence, sort_keys=True) + '\n')


if __name__ == '__main__':
    main()
