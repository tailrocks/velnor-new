# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import importlib.util
import json
import re
import subprocess
import os
import stat
from pathlib import Path
import shutil
import tempfile
import time
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))


def load_watch(source):
    spec = importlib.util.spec_from_file_location('release_bootstrap_watch_fixture', source / 'release_bootstrap_watch.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class RootWatch:
    def __init__(self, bus, lifecycle, source, seconds=1200, operation='publish'):
        self.directory = Path(tempfile.mkdtemp(prefix='velnor-publisher-', dir='/run'))
        self.directory.chmod(0o700)
        self.deadline = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + seconds * 1_000_000_000
        try:
            self.watch = load_watch(source).start(
                bus, lifecycle, str(self.directory), self.deadline, operation,
                (source / 'release_launcher_watch.py').read_bytes(),
                (source / 'release_supervisor_claim.py').read_bytes())
            (self.directory / 'fixture-watch.json').write_text(json.dumps({
                'unit': self.watch.data['unit'], 'invocation': self.watch.data['invocation'],
                'device': self.directory.stat().st_dev, 'inode': self.directory.stat().st_ino}))
            (self.directory / 'fixture-watch.json').chmod(0o600)
        except BaseException:
            shutil.rmtree(self.directory)
            raise

    @property
    def authority(self):
        return self.watch.authority

    @property
    def unit(self):
        return self.watch.data['unit']

    def close(self):
        self.watch.close(self.deadline)

    def __enter__(self):
        return self

    def __exit__(self, *_error):
        self.close()


def snapshot_roots():
    return set(Path('/run').glob('velnor-publisher-*/fixture-watch.json'))


def cleanup_orphans(before):
    for record in snapshot_roots() - before:
        descriptor = os.open(record, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            info = os.fstat(descriptor)
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                    or stat.S_IMODE(info.st_mode) != 0o600 or not 0 < info.st_size <= 4096):
                raise RuntimeError('fixture watch record changed')
            identity = json.loads(os.read(descriptor, 4097))
        finally:
            os.close(descriptor)
        directory = record.parent.stat(follow_symlinks=False)
        if (not stat.S_ISDIR(directory.st_mode) or directory.st_uid != 0
                or stat.S_IMODE(directory.st_mode) != 0o700
                or (directory.st_dev, directory.st_ino) != (identity['device'], identity['inode'])):
            raise RuntimeError('fixture watch root changed')
        unit = identity['unit']
        invocation = identity['invocation']
        if (set(identity) != {'unit', 'invocation', 'device', 'inode'}
                or not re.fullmatch(r'velnor-release-[a-f0-9]{32}\.service', unit)
                or not re.fullmatch(r'[a-f0-9]{32}', invocation)):
            raise RuntimeError('invalid fixture watch identity')
        result = subprocess.run(['/usr/bin/systemctl', 'show', unit, '--property=InvocationID',
            '--property=ActiveState', '--property=Job', '--property=ControlGroup'],
            check=True, capture_output=True, text=True, timeout=10)
        state = dict(line.split('=', 1) for line in result.stdout.splitlines())
        group = Path('/sys/fs/cgroup/system.slice') / unit
        if (state.get('InvocationID') != invocation or state.get('ActiveState') not in ('inactive', 'failed')
                or state.get('Job') or state.get('ControlGroup') or group.exists()):
            raise RuntimeError('fixture orphan watch cleanup unproved')
        dropin = Path('/run/systemd/system') / (unit + '.d')
        if set(path.name for path in dropin.iterdir()) != {'deadline.conf'}:
            raise RuntimeError('fixture watch staging changed')
        (dropin / 'deadline.conf').unlink()
        dropin.rmdir()
        shutil.rmtree(record.parent)
