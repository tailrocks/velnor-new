# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import ctypes
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading

shell, source, relative = sys.argv[1:]
config_dir = Path('/proc/self/fd')
fd = os.open(config_dir, os.O_RDONLY | os.O_DIRECTORY)
buf = ctypes.create_string_buffer(256)
assert ctypes.CDLL(None).fstatfs(fd, ctypes.byref(buf)) == 0
os.close(fd)
assert ctypes.c_long.from_buffer(buf).value == 0x9FA0
for name in ('config.yml', 'hosts.yml', '.config', '.local'):
    path = config_dir / name
    assert not path.exists()
    for operation in (lambda: os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600),
                      lambda: os.symlink('/tmp', path)):
        try:
            operation()
        except OSError:
            pass
        else:
            raise AssertionError('procfs allowed a named entry')
    assert not path.exists()

with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    env = {'PATH': '/usr/bin:/bin', 'HOME': temp, 'GH_CONFIG_DIR': temp,
           'TMPDIR': temp, 'VELNOR_VERIFIED_GH_DATA_DIR': source,
           'GH_TOKEN': 'offline-fixture'}
    def run(args, data=None):
        return subprocess.run(args, env=env, input=data, capture_output=True, timeout=30)
    sentinel = root / 'executed'
    direct = run([str(Path(source) / relative),
                  'alias', 'set', 'custodyprobe', '!touch "$TMPDIR/executed"'])
    assert direct.returncode == 0, direct.stderr.decode()
    assert run([str(Path(source) / relative), 'custodyprobe']).returncode == 0
    assert sentinel.exists()
    sentinel.unlink()
    def launch(command, data=None):
        return run(['/bin/sh', '-c', shell + '\n' + command], data)
    assert launch('gh alias list').returncode == 0
    assert launch('gh custodyprobe').returncode != 0
    assert not sentinel.exists()
    credential = launch('gh auth git-credential get', b'protocol=https\nhost=github.com\n\n')
    assert credential.returncode == 0
    assert b'offline-fixture' in credential.stdout
    assert not credential.stderr
    stop = threading.Event()
    def attacker():
        while not stop.is_set():
            for name in ('config.yml', 'hosts.yml'):
                try:
                    os.symlink(str(root / name), str(config_dir / name))
                except OSError:
                    pass
    thread = threading.Thread(target=attacker)
    thread.start()
    try:
        for _ in range(5):
            assert launch('gh custodyprobe').returncode != 0
            assert not sentinel.exists()
    finally:
        stop.set()
        thread.join()
