"""Actual raw-observation primitives; no Foundation, SDK or archive qualification."""
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest

sys.path[:0] = [str(Path(__file__).resolve().parents[1] / "src"),
               str(Path(__file__).resolve().parents[2] / "velnor-actions-orchestrator" / "src")]
from source_intent_cold_common import ColdSourceIntent
from source_archive_inventory import _inventory
from source_archive_inventory_common import InventoryError, _ORIGINAL_OBSERVATION, prefixes
from source_archive_inventory_fs import root_descriptor
from source_archive_inventory_leaf import HardlinkInventory
from source_archive_inventory_original import _original_inventory
from source_archive_inventory_walk import collect, walk


def observe_primitives(root, roots):
    """Exercise raw reader primitives; this helper mints no closed context."""
    descriptor = root_descriptor(str(root))
    try:
        return _original_inventory(str(root), roots, descriptor)
    finally:
        os.close(descriptor)


class ActualOriginalPrimitiveTests(unittest.TestCase):
    """No metadata masks, mocked readers, or adopted profiles."""
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        (self.root / "a").mkdir()
        (self.root / "b").mkdir()
        self.file = self.root / "a" / "file"
        self.file.write_bytes(b"raw original")
        self.roots = ("a", "b")

    def tearDown(self):
        self.temporary.cleanup()

    def observation(self):
        return observe_primitives(self.root, self.roots)

    def test_actual_full_raw_metadata_schema_and_bytes(self):
        result = self.observation()
        record = json.loads(result.canonical_bytes)
        self.assertEqual(record["schema"], 3)
        self.assertEqual((result.files, result.bytes), (1, 12))
        self.assertTrue(all("metadata" in entry and "hardlink" in entry
                            for entry in record["entries"]))
        self.assertTrue(all(set(entry["local"]) == {"dev", "ino", "uid", "gid", "mtime_ns", "ctime_ns"}
                            for entry in record["entries"]))

    def test_actual_device_inode_bind_every_object_and_owner(self):
        (self.root / "b" / "link").symlink_to("../a/file")
        record = json.loads(self.observation().canonical_bytes)
        for entry in record["entries"]:
            observed = (self.root / entry["path"]).lstat()
            self.assertEqual((entry["local"]["dev"], entry["local"]["ino"]),
                             (observed.st_dev, observed.st_ino))
        owner = self.root.stat()
        self.assertEqual((record["owner"]["local"]["dev"],
                          record["owner"]["local"]["ino"]), (owner.st_dev, owner.st_ino))

    def test_actual_equal_content_mode_mtime_inode_replacement_changes_original(self):
        before = self.observation()
        metadata = self.file.stat()
        replacement = self.root / "a" / "replacement"
        replacement.write_bytes(self.file.read_bytes())
        replacement.chmod(metadata.st_mode & 0o7777)
        os.utime(replacement, ns=(metadata.st_atime_ns, metadata.st_mtime_ns))
        self.assertNotEqual(replacement.stat().st_ino, metadata.st_ino)
        replacement.replace(self.file)
        after = self.observation()
        first, second = self.file_record(before), self.file_record(after)
        self.assertEqual((first["sha256"], first["mode"], first["local"]["mtime_ns"]),
                         (second["sha256"], second["mode"], second["local"]["mtime_ns"]))
        self.assertNotEqual(first["local"]["ino"], second["local"]["ino"])
        self.assertNotEqual(before.digest, after.digest)
        # ctime cannot be restored through filesystem APIs; explicit inode fields
        # independently preserve identity even when mutable fields match.

    def file_record(self, result, path="a/file"):
        return next(entry for entry in json.loads(result.canonical_bytes)["entries"]
                    if entry["path"] == path)

    def test_actual_leaf_mtime_only_changes_original(self):
        before = self.observation()
        metadata = self.file.stat()
        os.utime(self.file, ns=(metadata.st_atime_ns, metadata.st_mtime_ns + 1000000000))
        after = self.observation()
        self.assertNotEqual(before.digest, after.digest)
        self.assertEqual(self.file_record(after)["local"]["mtime_ns"], self.file.stat().st_mtime_ns)
        self.assertNotIn("atime_ns", self.file_record(after)["local"])

    def test_actual_directory_and_owner_time_changes_original(self):
        before = self.observation()
        path = self.root / "a"
        metadata = path.stat()
        os.utime(path, ns=(metadata.st_atime_ns, metadata.st_mtime_ns + 1000000000))
        directory = self.observation()
        self.assertNotEqual(before.digest, directory.digest)
        metadata = self.root.stat()
        os.utime(self.root, ns=(metadata.st_atime_ns, metadata.st_mtime_ns + 1000000000))
        owner = self.observation()
        self.assertNotEqual(directory.digest, owner.digest)
        self.assertEqual(json.loads(owner.canonical_bytes)["owner"]["local"]["mtime_ns"],
                         self.root.stat().st_mtime_ns)

    def test_actual_symlink_time_changes_original(self):
        link = self.root / "a" / "link"
        link.symlink_to("file")
        before = self.observation()
        metadata = link.lstat()
        os.utime(link, ns=(metadata.st_atime_ns, metadata.st_mtime_ns + 1000000000),
                 follow_symlinks=False)
        after = self.observation()
        self.assertNotEqual(before.digest, after.digest)
        self.assertEqual(self.file_record(after, "a/link")["local"]["mtime_ns"], link.lstat().st_mtime_ns)

    def test_actual_leaf_gid_changes_original(self):
        before = self.observation()
        groups = [group for group in os.getgroups() if group != self.file.stat().st_gid]
        if os.geteuid() != 0 and not groups:
            self.skipTest("no permitted alternate group; no GID mutation proof on this host")
        group = 5678 if os.geteuid() == 0 else groups[0]
        os.chown(self.file, -1, group, follow_symlinks=False)
        after = self.observation()
        self.assertNotEqual(before.digest, after.digest)
        self.assertEqual(self.file_record(after)["local"]["gid"], group)

    def test_actual_leaf_uid_changes_original(self):
        if os.geteuid() != 0:
            self.skipTest("UID mutation needs privilege; no UID mutation proof on this host")
        before = self.observation()
        os.chown(self.file, 1234, -1, follow_symlinks=False)
        after = self.observation()
        self.assertNotEqual(before.digest, after.digest)
        self.assertEqual(self.file_record(after)["local"]["uid"], 1234)

    def test_actual_xattr_value_changes_original_digest(self):
        attribute = "com.apple.velnor-original-test" if sys.platform == "darwin" else "user.velnor-original"
        before = self.observation()
        self.set_attribute(attribute, "first")
        first = self.observation()
        self.set_attribute(attribute, "second")
        second = self.observation()
        self.assertNotEqual(before.digest, first.digest)
        self.assertNotEqual(first.digest, second.digest)
        entries = json.loads(second.canonical_bytes)["entries"]
        self.assertTrue(next(entry for entry in entries if entry["path"] == "a/file")["metadata"])

    def set_attribute(self, attribute, value, path=None):
        path = self.file if path is None else path
        if sys.platform == "darwin":
            subprocess.run(["/usr/bin/xattr", "-w", attribute, value, str(path)], check=True)
        else:
            os.setxattr(path, attribute, value.encode("ascii"), follow_symlinks=False)

    def test_actual_owner_only_xattr_changes_original_digest(self):
        attribute = "com.apple.velnor-owner-test" if sys.platform == "darwin" else "user.velnor-owner"
        before = self.observation()
        self.set_attribute(attribute, "first", self.root)
        first = self.observation()
        self.set_attribute(attribute, "second", self.root)
        second = self.observation()
        self.assertNotEqual(before.digest, first.digest)
        self.assertNotEqual(first.digest, second.digest)
        self.assertTrue(json.loads(second.canonical_bytes)["owner"]["metadata"])

    def test_actual_owner_acl_changes_original_digest(self):
        before = self.observation()
        if sys.platform == "darwin":
            subprocess.run(["/bin/chmod", "+a", "everyone deny delete", str(self.root)], check=True)
            try:
                self.assertNotEqual(before.digest, self.observation().digest)
            finally:
                subprocess.run(["/bin/chmod", "-a", "everyone deny delete", str(self.root)], check=True)
        else:
            executable = shutil.which("setfacl")
            if executable is None:
                self.skipTest("setfacl unavailable; no ACL mutation proof on this host")
            subprocess.run([executable, "-m", "u:65534:r-x", str(self.root)], check=True)
            self.assertNotEqual(before.digest, self.observation().digest)

    def test_actual_owner_nodump_flag_changes_original_digest(self):
        before = self.observation()
        if sys.platform == "darwin":
            executable = shutil.which("chflags")
            if executable is None:
                self.skipTest("chflags unavailable; no flag mutation proof on this host")
            subprocess.run([executable, "nodump", str(self.root)], check=True)
        else:
            executable = shutil.which("chattr")
            if executable is None:
                self.skipTest("chattr unavailable; no flag mutation proof on this host")
            subprocess.run([executable, "+d", str(self.root)], check=True)
        self.assertNotEqual(before.digest, self.observation().digest)

    def test_actual_hardlink_topology_differs_from_copied_equal_bytes(self):
        alias = self.root / "b" / "alias"
        os.link(self.file, alias)
        linked = self.observation()
        entries = json.loads(linked.canonical_bytes)["entries"]
        identity = {"representative": "a/file", "count": 2}
        for entry in entries:
            if entry["kind"] == "file":
                self.assertEqual(entry["hardlink"], identity)
        alias.unlink()
        alias.write_bytes(self.file.read_bytes())
        self.assertNotEqual(linked.digest, self.observation().digest)

    def test_actual_external_hardlink_remains_rejected(self):
        os.link(self.file, self.root / "outside-selected-roots")
        with self.assertRaisesRegex(InventoryError, "payload_hardlink_external_or_missing"):
            self.observation()

    def test_actual_symlink_hardlink_topology_split_changes_original(self):
        first, alias = self.root / "a" / "link", self.root / "a" / "alias"
        first.symlink_to("file")
        os.link(first, alias, follow_symlinks=False)
        linked = self.observation()
        entries = json.loads(linked.canonical_bytes)["entries"]
        for entry in entries:
            if entry["kind"] == "symlink":
                self.assertEqual(entry["hardlink"], {"representative": "a/alias", "count": 2})
        alias.unlink()
        alias.symlink_to("file")
        self.assertNotEqual(linked.digest, self.observation().digest)

    def test_actual_external_symlink_hardlink_alias_rejected(self):
        link = self.root / "a" / "link"
        link.symlink_to("file")
        os.link(link, self.root / "unselected-alias", follow_symlinks=False)
        with self.assertRaisesRegex(InventoryError, "payload_hardlink_external_or_missing"):
            self.observation()

    def test_actual_absolute_direct_link_preserves_raw_original_target(self):
        raw = str(self.file)
        (self.root / "b" / "link").symlink_to(raw)
        result = self.observation()
        entries = json.loads(result.canonical_bytes)["entries"]
        self.assertEqual(next(entry for entry in entries if entry["kind"] == "symlink")["target"], raw)
        # The archive reader still rejects raw absolute targets or filesystem metadata.
        with self.assertRaisesRegex(InventoryError, "payload_(symlink_escape|unsupported_metadata)"):
            _inventory(str(self.root), self.roots)

    def test_actual_link_chain_and_unselected_absolute_target_reject(self):
        (self.root / "a" / "indirect").symlink_to("file")
        link = self.root / "b" / "link"
        link.symlink_to(str(self.root / "a" / "indirect"))
        with self.assertRaisesRegex(InventoryError, "payload_symlink_uncontained"):
            self.observation()
        link.unlink()
        (self.root / "unselected").write_bytes(b"outside union")
        link.symlink_to(str(self.root / "unselected"))
        with self.assertRaisesRegex(InventoryError, "payload_symlink_uncontained"):
            self.observation()

    def test_actual_appledouble_and_special_mode_are_recorded_only(self):
        header = struct.pack(">II16sH", 0x00051607, 0x00020000, b"\0" * 16, 1)
        self.file.write_bytes(header + struct.pack(">III", 2, 38, 1) + b"x")
        self.file.chmod(0o4700)
        result = self.observation()
        entry = next(entry for entry in json.loads(result.canonical_bytes)["entries"]
                     if entry["path"] == "a/file")
        self.assertEqual(entry["mode"], 0o4700)
        self.assertEqual(result.bytes, 39)

    def test_closed_entrypoint_rejects_data_and_unsealed_context(self):
        from source_intent_cold_context import FreshColdInstallationContext
        from source_intent_cold_manifest import source_original_inventory
        for context in (None, {"qualified": True}, object()):
            with self.assertRaisesRegex(ColdSourceIntent, "cold_manifest_context_authority"):
                source_original_inventory(context)
        unsealed = object.__new__(FreshColdInstallationContext)
        with self.assertRaisesRegex(ColdSourceIntent, "cold_manifest_context_authority"):
            source_original_inventory(unsealed)

    def test_actual_relative_parent_cannot_erase_linked_ancestor(self):
        from source_archive_inventory_common import links
        (self.root / "file").write_bytes(b"unobserved outside selected roots")
        (self.root / "a" / "dlink").symlink_to("../b")
        (self.root / "a" / "link").symlink_to("dlink/../file")
        self.assertEqual((self.root / "a" / "link").read_bytes(),
                         b"unobserved outside selected roots")
        with self.assertRaisesRegex(InventoryError, "payload_symlink_ancestor"):
            self.observation()
        descriptor = root_descriptor(str(self.root))
        try:
            entries, _size = collect(walk(str(self.root), descriptor, HardlinkInventory(),
                                          roots=self.roots, observation=_ORIGINAL_OBSERVATION))
            with self.assertRaisesRegex(InventoryError, "payload_symlink_ancestor"):
                links(entries, prefixes(self.roots))
        finally:
            os.close(descriptor)

    def test_actual_absolute_parent_cannot_erase_linked_ancestor(self):
        (self.root / "file").write_bytes(b"unobserved outside selected roots")
        (self.root / "a" / "dlink").symlink_to("../b")
        (self.root / "a" / "link").symlink_to(str(self.root / "a" / "dlink") + "/../file")
        self.assertEqual((self.root / "a" / "link").read_bytes(),
                         b"unobserved outside selected roots")
        with self.assertRaisesRegex(InventoryError, "payload_symlink_ancestor"):
            self.observation()


if __name__ == "__main__":
    unittest.main()
