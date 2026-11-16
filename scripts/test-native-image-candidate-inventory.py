# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "native_inventory", Path(__file__).with_name("native-image-candidate-inventory.py")
)
inventory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inventory)


class InventoryCliTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.source = "crates/velnor-actions-cli/src/main.rs"
        manifest = "crates/velnor-actions-cli/Cargo.toml"
        files = {
            self.source: "fn main() {}\n", manifest: "fixture manifest\n",
            "Cargo.toml": "fixture workspace\n", "Cargo.lock": "fixture lock\n",
            inventory.GENERATOR: "fixture generator\n", inventory.INVENTORY: "",
            inventory.SUPPLEMENTAL: "mise.toml\n", "mise.toml": "fixture supplemental\n",
        }
        self.expected = sorted(files)
        files[inventory.INVENTORY] = "".join(f"{path}\n" for path in self.expected)
        for path, contents in files.items():
            self.write(path, contents)
        self.metadata, self.artifacts = self.root / "metadata.json", self.root / "artifacts.jsonl"
        target = self.root / "target"
        target.mkdir()
        executable = target / "velnor-actions"
        self.dep_info = target / "velnor-actions.d"
        self.dep_info.write_text(f"{executable}: {self.root / self.source}\n")
        package = "fixture-cli"
        self.metadata.write_text(json.dumps({
            "workspace_root": str(self.root), "target_directory": str(target),
            "packages": [{
                "id": package, "name": "velnor-actions-cli", "source": None,
                "manifest_path": str(self.root / manifest)}],
            "resolve": {"nodes": [{"id": package, "deps": []}]},
        }))
        self.artifacts.write_text(json.dumps({
            "reason": "compiler-artifact", "package_id": package,
            "target": {"name": "velnor-actions", "kind": ["bin"]}, "filenames": [str(executable)],
        }) + "\n")
        self.git("init", "--quiet")
        self.git("add", ".")

    def write(self, relative, contents):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)

    def git(self, *arguments):
        subprocess.check_output(["git", "-C", str(self.root), *arguments], stderr=subprocess.PIPE)

    def assert_cli(self, mode="--check", status=0):
        result = subprocess.run([
            sys.executable, str(Path(inventory.__file__).resolve()), mode,
            "--root", str(self.root), "--metadata", str(self.metadata),
            "--artifacts", str(self.artifacts), "--target", inventory.TARGET,
        ], capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, status, result.stderr)
        return result

    def test_matching_inventory_passes_without_rewriting(self):
        path = self.root / inventory.INVENTORY
        before = path.read_bytes()
        self.assert_cli()
        self.assertEqual(path.read_bytes(), before)

    def test_new_tracked_compiler_input_fails_until_inventory_is_regenerated(self):
        added = "crates/velnor-actions-cli/src/added.rs"
        self.write(added, "pub fn added() {}\n")
        self.git("add", added)
        self.dep_info.write_text(self.dep_info.read_text().rstrip() + f" {self.root / added}\n")
        result = self.assert_cli(status=1)
        self.assertIn(added, result.stderr)
        self.assert_cli("--write")
        self.assert_cli()
        self.assertIn(added, (self.root / inventory.INVENTORY).read_text().splitlines())

    def test_missing_inventory_line_fails(self):
        self.write(inventory.INVENTORY, "".join(
            f"{path}\n" for path in self.expected if path != "mise.toml"
        ))
        result = self.assert_cli(status=1)
        self.assertIn("mise.toml", result.stderr)


class DepInfoTests(unittest.TestCase):
    def test_first_rule_preserves_escaped_paths_and_ignores_later_records(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "artifact.d"
            for newline in ("\n", "\r\n"):
                with self.subTest(newline=repr(newline)):
                    contents = (
                        r"target\:name: src/with\ space.rs path\\name.rs " + "\\"
                        + newline
                        + " nested/other.rs"
                        + newline
                        + newline
                        + "phony: injected.rs"
                        + newline
                        + "# env-dep:VELNOR_SOURCE_SHA=ignored"
                        + newline
                    )
                    path.write_bytes(contents.encode())
                    self.assertEqual(
                        inventory.make_prerequisites(path),
                        ["src/with space.rs", r"path\name.rs", "nested/other.rs"],
                    )



if __name__ == "__main__":
    unittest.main()
