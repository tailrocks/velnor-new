# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
from pathlib import Path
import shlex
import shutil
import socket
import sys
import tempfile
import time
import types
import unittest
from unittest.mock import patch


sys.path.insert(0, str(Path(__file__).parent))
from release_publish_entry_test import context
from release_publish_entry_systemd_test import entry


class PublisherEntryOfflineTest(unittest.TestCase):
    def test_main_passes_credential_to_real_bootstrap(self):
        self.assertEqual(os.geteuid(), 0)
        shared = Path('/run/systemd/system/velnor-release-.service.d')
        shared.mkdir(mode=0o700)
        dropin = shared / 'offline-fixture.conf'
        safe = True
        try:
            with dropin.open('x', encoding='ascii') as output:
                output.write('[Service]\nPrivateNetwork=yes\n')
            with tempfile.TemporaryDirectory(prefix='velnor-net-proof-', dir='/run') as root:
                listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                listener.bind(('127.0.0.1', 0))
                listener.listen(1)
                listener.settimeout(1)
                marker = Path(root) / 'isolated'
                code = ('import os,socket,sys;assert set(os.listdir("/sys/class/net"))=={"lo"};'
                        's=socket.socket();s.settimeout(0.5);'
                        'result=s.connect_ex(("127.0.0.1",int(sys.argv[1])));s.close();'
                        'assert result!=0;open(sys.argv[2],"w").write("isolated")')
                script = '/usr/bin/python3 -I -S -c ' + shlex.quote(code) + ' ' + str(listener.getsockname()[1]) + ' ' + shlex.quote(str(marker))
                deadline = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 60_000_000_000
                safe = False
                try:
                    entry['acquire'](root, script, {'PATH': '/usr/bin:/bin', 'HOME': root}, deadline)
                except Exception as error:
                    safe = str(error) == 'unit failed after verified cleanup'
                    raise
                safe = True
                self.assertEqual(marker.read_text(), 'isolated')
                with self.assertRaises(TimeoutError):
                    listener.accept()
                listener.close()
            before = set(Path('/run').glob('velnor-publisher-*'))
            arguments = ['entry', 'trusted-publishing', '0.1.73', 'a' * 40, 'b' * 64]
            safe = False
            with patch.dict(os.environ, context('trusted-publishing'), clear=True), \
                    patch.object(os, 'uname', return_value=types.SimpleNamespace(machine='x86_64')), \
                    patch.object(sys, 'argv', arguments):
                with self.assertRaises(Exception) as failure:
                    entry['main']()
            self.assertEqual(str(failure.exception), 'unit failed after verified cleanup')
            safe = True
            created = set(Path('/run').glob('velnor-publisher-*')) - before
            self.assertEqual(len(created), 1)
            directory = created.pop()
            try:
                self.assertEqual(directory.stat().st_uid, 0)
                self.assertTrue((directory / 'velnor/bootstrap').is_dir())
            finally:
                shutil.rmtree(directory)
        finally:
            if safe:
                dropin.unlink()
                shared.rmdir()


if __name__ == '__main__':
    unittest.main()
