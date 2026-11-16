# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
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
            self.source: "fn main() {}\n",
            manifest: (
                '[package]\nname = "velnor-actions-cli"\nversion = "0.1.0"\nedition = "2024"\n'
                '[[bin]]\nname = "velnor-actions"\npath = "src/main.rs"\n'
            ),
            "Cargo.toml": '[workspace]\nmembers = ["crates/velnor-actions-cli"]\nresolver = "3"\n',
            "Cargo.lock": 'version = 4\n\n[[package]]\nname = "velnor-actions-cli"\nversion = "0.1.0"\n',
            inventory.GENERATOR: Path(inventory.__file__).read_text(), inventory.INVENTORY: "",
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

    def test_configured_task_rejects_new_tracked_compiler_input(self):
        repository = Path(__file__).resolve().parents[1]
        toolchain = Path(os.environ.get("RUSTUP_TOOLCHAIN", ""))
        if not toolchain.is_absolute():
            toolchain = Path(subprocess.check_output(
                ["rustc", "--print", "sysroot"], cwd=repository,
                env=dict(os.environ, RUSTUP_AUTO_INSTALL="0"), text=True,
            ).strip())
        binaries = toolchain / "bin"
        task = tomllib.loads((repository / "mise.toml").read_text())["tasks"]["check-native-image-inventory"]["run"]
        work = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        environment = {
            key: value for key, value in os.environ.items()
            if not key.startswith("CARGO_") and key not in (
                "RUSTFLAGS", "RUSTDOCFLAGS", "RUSTC", "RUSTDOC",
                "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER",
            )
        }
        environment.update(
            PATH=os.pathsep.join((str(binaries), str(Path(sys.executable).parent), os.environ["PATH"])),
            RUSTC=str(binaries / "rustc"), RUSTUP_AUTO_INSTALL="0", CARGO_NET_OFFLINE="true",
            CARGO_HOME=str(work / "cargo-home"), CARGO_TARGET_DIR=str(work / "target"),
        )
        script = (
            "uname() { case \"$1\" in -s) printf 'Linux\\n';; -m) printf 'x86_64\\n';; esac; }\n"
            + task
        )
        result = subprocess.run(
            ["/bin/sh", "-c", script], cwd=self.root, env=environment,
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        added = "crates/velnor-actions-cli/src/added.rs"
        self.write(added, "")
        self.write(self.source, "mod added;\nfn main() {}\n")
        self.git("add", added, self.source)
        result = subprocess.run(
            ["/bin/sh", "-c", script], cwd=self.root, env=environment,
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(added, result.stderr)


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
