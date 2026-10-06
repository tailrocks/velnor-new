"""Canonical root leaves disappear; receipt-control and Source3 remain."""
import os
import pathlib
import tempfile
import unittest

DIRECTORY = pathlib.Path(__file__).parent
namespace = {}
exec((DIRECTORY / 'catalog_native_health.py').read_text(), namespace)
exec((DIRECTORY / 'catalog_native_root_clear.py').read_text(), namespace)
clear = namespace['clear_selected_roots']
FULL = ('mise', 'rustup', 'cargo/bin', 'cargo/.crates.toml', 'cargo/.crates2.json')


class RootClear(unittest.TestCase):
    def setUp(self):
        self.sandbox = tempfile.TemporaryDirectory()
        self.temp = pathlib.Path(self.sandbox.name).resolve()
        self.owned = self.temp / 'velnor'
        self.owned.mkdir(mode=0o700)

    def tearDown(self):
        self.sandbox.cleanup()

    def test_full_leaves_absent_sources_and_control_preserved(self):
        for name in ('mise/bin', 'rustup/toolchains', 'cargo/bin',
                     'cargo/registry/index', 'cargo/registry/cache', 'cargo/git/db',
                     'cache-receipts/control'):
            directory = self.owned / name
            directory.mkdir(parents=True)
            (directory / 'payload').write_text(name)
        for name in ('.crates.toml', '.crates2.json'):
            (self.owned / 'cargo' / name).write_text('install metadata')
        clear(str(self.temp), FULL)
        for root in FULL:
            self.assertFalse((self.owned / root).exists())
        for name in ('cargo/registry/index', 'cargo/registry/cache', 'cargo/git/db',
                     'cache-receipts/control'):
            self.assertEqual((self.owned / name / 'payload').read_text(), name)

    def test_selected_leaf_link_unlinked_without_following(self):
        outside = self.temp / 'outside'
        outside.mkdir()
        (outside / 'canary').write_text('preserve')
        (self.owned / 'mise').symlink_to(outside, target_is_directory=True)
        clear(str(self.temp), ('mise',))
        self.assertFalse((self.owned / 'mise').is_symlink())
        self.assertEqual((outside / 'canary').read_text(), 'preserve')

    def test_missing_namespace_created_safely(self):
        self.owned.rmdir()
        clear(str(self.temp), FULL)
        self.assertTrue(self.owned.is_dir())
        self.assertEqual(self.owned.stat().st_mode & 0o777, 0o700)

    def test_missing_intermediate_parent_accepted(self):
        clear(str(self.temp), FULL)
        self.assertEqual(list(self.owned.iterdir()), [])

    def test_linked_namespace_rejected(self):
        self.owned.rmdir()
        self.owned.symlink_to(self.temp, target_is_directory=True)
        with self.assertRaises(OSError):
            clear(str(self.temp), FULL)

    def test_linked_root_parent_rejected(self):
        (self.owned / 'cargo').symlink_to(self.temp, target_is_directory=True)
        with self.assertRaises(OSError):
            clear(str(self.temp), FULL)

    def test_writable_owned_parent_rejected(self):
        cargo = self.owned / 'cargo'
        cargo.mkdir()
        cargo.chmod(0o777)
        with self.assertRaisesRegex(ValueError, 'parent_owner'):
            clear(str(self.temp), ('cargo/bin',))

    def test_traversal_root_rejected(self):
        with self.assertRaisesRegex(ValueError, 'selected_root'):
            clear(str(self.temp), ('../outside',))

    def test_fifo_root_unlinked_without_reading(self):
        os.mkfifo(self.owned / 'mise')
        clear(str(self.temp), ('mise',))
        self.assertFalse((self.owned / 'mise').exists())


if __name__ == '__main__':
    unittest.main()
