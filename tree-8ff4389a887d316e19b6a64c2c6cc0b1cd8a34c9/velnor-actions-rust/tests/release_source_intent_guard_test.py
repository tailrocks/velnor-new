"""Pure source policy tests; never invoke Cargo, Git, or network."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


MODULE = Path(__file__).resolve().parents[1] / "src/release_source_intent_guard.py"
NAMESPACE = {"__name__": "source_intent_guard_tests"}
exec(compile(MODULE.read_bytes(), str(MODULE), "exec"), NAMESPACE)
Error = NAMESPACE["SourceIntentError"]
guard = NAMESPACE["guard_source_intent"]


class SourceIntentGuardTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="velnor-source-policy-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve() / "source"
        self.root.mkdir()
        self.write("Cargo.toml", '[package]\nname="proof"\nversion="1.0.0"\n')

    def write(self, path, content):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")

    def test_plain_registry_source_allowed(self):
        self.write("Cargo.lock", '[[package]]\nname="proof"\nversion="1.0.0"\n'
                   '[[package]]\nname="dep"\nversion="1.0.0"\n'
                   'source="registry+https://github.com/rust-lang/crates.io-index"\n')
        self.assertEqual(guard(self.root), self.root)

    def test_enclosed_parent_dependency_allowed(self):
        self.write("crates/a/Cargo.toml", '[package]\nname="a"\nversion="1.0.0"\n'
                   'workspace="../.."\n[dependencies]\nb={path="../b",version="1"}\n')
        self.write("crates/b/Cargo.toml", '[package]\nname="b"\nversion="1.0.0"\n')
        self.assertEqual(guard(self.root), self.root)

    def test_dependency_source_rejections(self):
        for dependency in ('{git="https://example.test/repo"}', '{registry="private"}',
                           '{registry-index="https://example.test/index"}',
                           '{path="../escape"}', '{path="/outside"}', '{artifact="bin"}'):
            with self.subTest(dependency=dependency):
                self.write("Cargo.toml", '[dependencies]\nbad=' + dependency + '\n')
                with self.assertRaises(Error):
                    guard(self.root)

    def test_workspace_and_target_sources_checked(self):
        for heading in ('workspace.dependencies', 'target.\'cfg(unix)\'.build-dependencies'):
            with self.subTest(heading=heading):
                self.write("Cargo.toml", '[' + heading + ']\nbad={git="https://bad"}\n')
                with self.assertRaises(Error):
                    guard(self.root)

    def test_lock_git_source_rejected_before_cargo(self):
        self.write("Cargo.lock", '[[package]]\nname="bad"\nsource="git+https://bad#123"\n')
        with self.assertRaises(Error):
            guard(self.root)

    def test_legacy_dependency_headings_rejected(self):
        for heading in ('build_dependencies', 'dev_dependencies',
                        'workspace.dev_dependencies',
                        'target.\'cfg(unix)\'.build_dependencies'):
            with self.subTest(heading=heading):
                self.write("Cargo.toml", '[' + heading + ']\nbad={git="https://bad"}\n')
                with self.assertRaises(Error):
                    guard(self.root)

    def test_filesystem_vectors_rejected(self):
        for relative in ("nested/.git/config", "nested/.GIT/config", ".cargo/config.toml",
                         "nested/.cargo/config", ".cargo_vcs_info.json"):
            with self.subTest(relative=relative):
                self.write(relative, "")
                with self.assertRaises(Error):
                    guard(self.root)
                (self.root / relative).unlink()
                if ".git" in relative.lower():
                    (self.root / relative).parent.rmdir()

    def test_symlink_rejected(self):
        (self.root / "linked").symlink_to(self.root / "Cargo.toml")
        with self.assertRaises(Error):
            guard(self.root)

    def test_ancestor_configuration_rejected(self):
        config = self.root.parent / ".cargo/config.toml"
        config.parent.mkdir()
        config.write_text("", encoding="utf-8")
        with self.assertRaises(Error):
            guard(self.root)

    def test_ancestor_workspace_rejected(self):
        (self.root.parent / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
        with self.assertRaisesRegex(Error, "ambient_workspace"):
            guard(self.root)

    def test_incomplete_walk_fails_closed(self):
        def failed_walk(*args, **kwargs):
            kwargs["onerror"](PermissionError("unreadable subtree"))
            return iter(())
        with patch.object(NAMESPACE["os"], "walk", side_effect=failed_walk):
            with self.assertRaisesRegex(Error, "incomplete_walk"):
                guard(self.root)

    def test_manifest_paths_cannot_escape(self):
        for content in ('[package]\nworkspace=".."\n', '[workspace]\nmembers=["../*"]\n',
                        '[package]\nreadme="../README.md"\n', '[lib]\npath="../lib.rs"\n',
                        '[[bin]]\npath="../main.rs"\n'):
            with self.subTest(content=content):
                self.write("Cargo.toml", content)
                with self.assertRaises(Error):
                    guard(self.root)

    def test_patch_replace_and_unstable_rejected(self):
        for content in ('[patch.crates-io]\n', '[replace]\n', 'cargo-features=["unstable"]\n'):
            with self.subTest(content=content):
                self.write("Cargo.toml", content)
                with self.assertRaises(Error):
                    guard(self.root)

    def test_malformed_toml_and_field_types_fail_closed(self):
        for content in ('[broken', 'package=1\n', 'workspace="bad"\n',
                        'bin=[1]\n', '[dependencies]\nbad=3\n',
                        '[workspace]\nmembers=1\n', '[target]\nfoo=1\n'):
            with self.subTest(content=content):
                self.write("Cargo.toml", content)
                with self.assertRaises(Error):
                    guard(self.root)

    def test_parent_recursive_glob_fails_explicitly(self):
        self.write("nested/Cargo.toml", '[workspace]\nmembers=["**/../.."]\n')
        with self.assertRaisesRegex(Error, "source_intent_parent_glob_unsupported"):
            guard(self.root)

    def test_include_exclude_patterns_remain_cargo_authority(self):
        self.write("Cargo.toml", '[package]\nname="proof"\nversion="1.0.0"\n'
                   'include=["/src/**", "!/tests/**"]\nexclude=["/tmp/**"]\n')
        self.assertEqual(guard(self.root), self.root)


if __name__ == "__main__":
    unittest.main()
