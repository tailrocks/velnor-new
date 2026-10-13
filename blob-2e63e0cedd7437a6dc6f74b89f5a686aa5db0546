"""Focused tests for the pinned Rust qualification receipt helper."""

from __future__ import annotations

import hashlib
import io
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

from qualify_rust_toolchain import (
    ensure_mbx_executor_unchanged,
    manifest_artifacts,
    mbx_exec_argv,
    mbx_executor_identity,
    run_mbx_probe,
    validate_mbx_version,
)
from rust_toolchain_tree import (
    QualificationError,
    canonical_tree_sha256,
    extract_archive,
    merge_component,
    safe_path,
    tree_entries,
)


class QualificationTests(unittest.TestCase):
    def test_candidate_probes_run_through_absolute_mbx_and_project_root(self) -> None:
        mbx = Path("/mise/installs/mr-boxington/1.23.0/bin/mbx")
        project_root = Path("/workspace/project")
        candidate = Path("/tmp/rust-prefix/bin/cargo")
        expected = [
            str(mbx),
            "exec",
            "--project-root",
            str(project_root),
            str(candidate),
            "--version",
        ]
        self.assertEqual(mbx_exec_argv(mbx, project_root, candidate, ("--version",)), expected)
        with patch("qualify_rust_toolchain.subprocess.run") as run:
            run_mbx_probe(mbx, project_root, candidate, ("--version",), {"HOME": "/tmp"})
        self.assertEqual(run.call_args.args[0], expected)
        self.assertTrue(run.call_args.kwargs["check"])
        self.assertEqual(run.call_args.kwargs["timeout"], 30)

    def test_mbx_probe_rejects_relative_paths_and_unpinned_executor_version(self) -> None:
        with self.assertRaises(QualificationError):
            mbx_exec_argv(Path("mbx"), Path("/workspace"), Path("/tmp/rustc"), ("-vV",))
        with self.assertRaises(QualificationError):
            validate_mbx_version("mbx 1.23.1\n", "1.23.0")
        validate_mbx_version("mbx 1.23.0\n", "1.23.0")

    def test_receipt_binds_exact_mbx_executable_bytes_and_path(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            mbx = Path(temporary) / "mbx"
            mbx.write_bytes(b"pinned MBX executable bytes\n")
            mbx.chmod(0o755)
            self.assertEqual(
                mbx_executor_identity(mbx, "1.23.0"),
                {
                    "id": "mr-boxington",
                    "version": "1.23.0",
                    "path": str(mbx),
                    "sha256": hashlib.sha256(b"pinned MBX executable bytes\n").hexdigest(),
                },
            )

    def test_qualification_rejects_mbx_replacement_after_version_check(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            mbx = Path(temporary) / "mbx"
            mbx.write_bytes(b"version-checked bytes\n")
            mbx.chmod(0o755)
            initial = mbx_executor_identity(mbx, "1.23.0")
            mbx.write_bytes(b"replacement bytes\n")
            replacement = mbx_executor_identity(mbx, "1.23.0")
            with self.assertRaises(QualificationError):
                ensure_mbx_executor_unchanged(initial, replacement)

    def test_manifest_selects_exact_component_archives(self) -> None:
        target = "aarch64-apple-darwin"
        names = {
            "cargo": f"cargo-1.99.0-{target}.tar.xz",
            "rustc": f"rustc-1.99.0-{target}.tar.xz",
            "rust-std": f"rust-std-1.99.0-{target}.tar.xz",
        }
        manifest = {
            "pkg": {
                package: {
                    "version": "0.100.0 (cargo commit)" if package == "cargo" else "1.99.0 (stable)",
                    "target": {
                        target: {
                            "available": True,
                            "xz_url": f"https://static.rust-lang.org/dist/{name}",
                            "xz_hash": hashlib.sha256(name.encode()).hexdigest(),
                        }
                    },
                }
                for package, name in names.items()
            }
        }
        artifacts = manifest_artifacts(manifest, target, "1.99.0")
        self.assertEqual({item["component"] for item in artifacts}, {
            "cargo", "rustc", f"rust-std-{target}"
        })
        self.assertEqual({item["url"].rsplit("/", 1)[-1] for item in artifacts}, set(names.values()))

    def test_manifest_rejects_untrusted_archive_host(self) -> None:
        target = "x86_64-unknown-linux-gnu"
        manifest = {
            "pkg": {
                package: {
                    "version": "1.99.0",
                    "target": {
                        target: {
                            "available": True,
                            "xz_url": f"https://attacker.invalid/{package}-1.99.0-{target}.tar.xz",
                            "xz_hash": "a" * 64,
                        }
                    },
                }
                for package in ("cargo", "rustc", "rust-std")
            }
        }
        with self.assertRaises(QualificationError):
            manifest_artifacts(manifest, target, "1.99.0")

    def test_install_projection_matches_canonical_tree_recipe(self) -> None:
        rows = [
            {"path": "bin", "kind": "directory"},
            {"path": "bin/rustc", "kind": "file", "sha256": "a" * 64, "executable": True},
        ]
        canonical = (
            b'[{"kind":"directory","path":"bin"},'
            b'{"executable":true,"kind":"file","path":"bin/rustc","sha256":"' + b"a" * 64 + b'"}]'
        )
        self.assertEqual(canonical_tree_sha256(rows), hashlib.sha256(canonical).hexdigest())

    def test_component_merge_and_archive_traversal_rejection(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            component = root / "archive"
            (component / "cargo" / "bin").mkdir(parents=True)
            (component / "components").write_text("cargo\n", encoding="utf-8")
            (component / "cargo" / "manifest.in").write_text("file:bin/cargo\n", encoding="utf-8")
            executable = component / "cargo" / "bin" / "cargo"
            executable.write_text("cargo payload\n", encoding="utf-8")
            executable.chmod(0o755)
            prefix = root / "prefix"
            prefix.mkdir()
            merge_component(component, "cargo", prefix)
            self.assertEqual((prefix / "bin" / "cargo").read_text(encoding="utf-8"), "cargo payload\n")
            self.assertEqual([entry["path"] for entry in tree_entries(prefix)], ["bin", "bin/cargo"])
            with self.assertRaises(QualificationError):
                safe_path("../outside")
            for invalid in ("bin:bad", "bin/has space", "bin/control\nchar"):
                with self.assertRaises(QualificationError):
                    safe_path(invalid)

    def test_archive_extractor_refuses_parent_traversal(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive_path = root / "malicious.tar.xz"
            with tarfile.open(archive_path, mode="w:xz") as archive:
                member = tarfile.TarInfo("../outside")
                payload = b"escape"
                member.size = len(payload)
                archive.addfile(member, io.BytesIO(payload))
            with self.assertRaises(QualificationError):
                extract_archive(archive_path, root / "out")


if __name__ == "__main__":
    unittest.main()
