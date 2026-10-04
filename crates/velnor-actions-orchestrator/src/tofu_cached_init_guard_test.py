"""Exercise root namespaces against the exact compiled cached-init body."""

import pathlib
import shlex
import subprocess
import tempfile
import unittest


class CachedInitRootGuardTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="velnor-tofu-guard-")
        self.addCleanup(self.directory.cleanup)
        self.base = pathlib.Path(self.directory.name)
        self.repo = self.base / "repo"
        self.repo.mkdir()
        self.outside = self.base / "outside"
        self.outside.mkdir()
        self.sentinel = self.outside / "sentinel"
        self.sentinel.write_text("untouched")
        (self.repo / "installs").symlink_to(self.outside, target_is_directory=True)
        (self.base / "alias").symlink_to(self.outside, target_is_directory=True)
        source = pathlib.Path(__file__).with_name("tofu_cached_init_source.sh").read_text()
        self.assertEqual(source.count('\nmain "$@"'), 1)
        self.engine = source.rsplit('\nmain "$@"', 1)[0]

    def check(self, command):
        result = subprocess.run(
            ["bash", "-c", self.engine + "\n" + command],
            cwd=self.repo,
            capture_output=True,
            text=True,
            timeout=10,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.sentinel.read_text(), "untouched")

    def test_tool_relative_path_ignores_unrelated_checkout_alias(self):
        self.check("relative_root_ok installs/http-opentofu/1.13.1/tofu")

    def test_project_root_rejects_checkout_alias(self):
        self.check("! relative_root_ok installs/http-opentofu/1.13.1/tofu 1")

    def test_absolute_tool_path_rejects_actual_ancestor_alias(self):
        self.check("! path_ok " + shlex.quote(str(self.base / "alias" / "tofu")))


if __name__ == "__main__":
    unittest.main()
