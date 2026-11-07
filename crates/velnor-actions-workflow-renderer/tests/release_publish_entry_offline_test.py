# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
from pathlib import Path
import shutil
import sys
import types
import unittest
from unittest.mock import patch


sys.path.insert(0, str(Path(__file__).parent))
from release_publish_entry_test import context
from release_publish_entry_systemd_test import entry, source_root
from release_watch_fixture import RootWatch


class PublisherEntryOfflineTest(unittest.TestCase):
    def test_main_passes_credential_to_real_bootstrap(self):
        self.assertEqual(os.geteuid(), 0)
        bus = entry["module"]("offline_bus", entry["MANAGER"])
        lifecycle = entry["module"]("offline_lifecycle", entry["LIFECYCLE"])
        before = set(Path('/run').glob('velnor-publisher-*'))
        arguments = ['entry', 'trusted-publishing', '0.1.73', 'a' * 40, 'b' * 64]
        handoff = RootWatch(bus, lifecycle, source_root, seconds=60)
        values = context('trusted-publishing')
        values['VELNOR_RELEASE_BUDGET'] = values['VELNOR_RELEASE_BUDGET'].rsplit(':', 1)[0] + ':' + str(handoff.deadline)
        values.update(handoff.watch.prepare_exec_environment())
        with patch.dict(os.environ, values, clear=True), \
                patch.object(os, 'uname', return_value=types.SimpleNamespace(machine='x86_64')), \
                patch.object(sys, 'argv', arguments):
            with self.assertRaises(Exception) as failure:
                entry['main']()
        handoff.watch.manager.close()
        self.assertEqual(str(failure.exception), 'unit failed after verified cleanup')
        created = set(Path('/run').glob('velnor-publisher-*')) - before
        self.assertEqual(len(created), 1)
        directory = created.pop()
        try:
            self.assertEqual(directory.stat().st_uid, 0)
            self.assertTrue((directory / 'velnor/bootstrap').is_dir())
        finally:
            shutil.rmtree(directory)



if __name__ == '__main__':
    unittest.main()
