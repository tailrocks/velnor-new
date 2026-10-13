"""Isolated source admission and mocked candidate build proof."""

import importlib.util
import gzip
from contextlib import redirect_stderr, redirect_stdout
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent))
import owned_tool_source as source
from owned_tool_git_fixture import fixture_ignored_blob


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parent / filename)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


builder = module("owned_builder", "build-owned-tool.py")
bootstrap = module("owned_bootstrap", "bootstrap-owned-tool-builder.py")
NATIVE_BINARY = b"\xcf\xfa\xed\xfe\x0c\x00\x00\x01\x00\x00\x00\x00\x02" + b"\0" * 19


def archive(entries):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w", format=tarfile.USTAR_FORMAT) as tar:
        for name, data, mode, link in entries:
            member = tarfile.TarInfo("source/" + name)
            member.mode = mode
            if link is not None:
                member.type, member.linkname = tarfile.SYMTYPE, link
            else:
                member.size = len(data)
            tar.addfile(member, None if link is not None else io.BytesIO(data))
    return output.getvalue()


def fixture(tool="mise"):
    return {"tool": tool, "version": "2026.10.6-owned-cargo-wrapper" if tool == "mise" else "1.21.1-velnor.1",
            "source_commit": "1" * 40, "source_tree": "2" * 40,
            "upstream_base_commit": source.BASES[tool][1],
            "archive_url": "https://github.com/tailrocks/velnor-new/releases/download/owned-source/source.tar",
            "receipt_url": "https://github.com/tailrocks/velnor-new/releases/download/owned-source/source-receipt.json",
            "patch_url": "https://github.com/tailrocks/velnor-new/releases/download/owned-source/base.patch",
            "archive_sha256": "a" * 64, "receipt_sha256": "b" * 64,
            "patch_sha256": "c" * 64, "lockfile_sha256": source.digest(b"lock"),
            "license_files": {"LICENSE": source.digest(b"license")}}


class SourceTests(unittest.TestCase):
    def test_all_official_bootstrap_rows_have_complete_native_authority(self):
        self.assertGreaterEqual(source.MAX_BOOTSTRAP_DOWNLOAD, 161_267_824)
        self.assertGreaterEqual(source.MAX_BOOTSTRAP_EXECUTABLE_BYTES, 140_066_264)
        for tool in ("mise", "mbx"):
            assets = source.official_assets(tool)
            self.assertEqual(set(assets), set(source.HOSTS))
            for target, asset in assets.items():
                with self.subTest(tool=tool, target=target):
                    for field in ("archive_sha256", "binary_sha256"):
                        source.check_hash(asset[field])
                        with self.assertRaises(ValueError):
                            source.check_hash(asset[field][:-1])
                    for field in ("source_commit", "source_tree"):
                        self.assertRegex(asset[field], r"^[0-9a-f]{40}$")
                        self.assertNotEqual(asset[field], "0" * 40)
                    self.assertIn(asset["format"], ("standalone", "tar.gz"))
                    member = "" if asset["format"] == "standalone" else (
                        "mise/bin/mise" if tool == "mise" else "mbx")
                    self.assertEqual(asset["binary_member"], member)

    def test_license_key_cannot_override_mandatory_lock_hash(self):
        spec = fixture()
        spec["lockfile_sha256"] = source.digest(b"wrong recorded lock")
        spec["license_files"]["Cargo.lock"] = source.digest(b"lock")
        data = archive([("Cargo.lock", b"lock", 0o644, None),
                        ("LICENSE", b"license", 0o644, None)])
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(source, "fetch", side_effect=[b"receipt", data, b"patch"]), \
                patch.object(source, "validate_receipt"), patch.object(source, "verify_patch"), \
                patch.object(source, "git_tree", return_value=spec["source_tree"]), \
                self.assertRaisesRegex(ValueError, "committed lock or license mismatch"):
            source.admit(spec, Path(temporary) / "source", {"PATH": os.environ["PATH"]})

    def test_descriptor_rejects_ambiguous_authority(self):
        spec = fixture()
        self.assertEqual(source.descriptor(json.dumps(spec), "f" * 40), spec)
        mutations = [{"unknown": True}, {"source_commit": "f" * 40},
                     {"version": "2026.10.4"}, {"version": "02026.10.0-owned-wrapper"},
                     {"version": "2026.10.0-velnor.0"}, {"archive_sha256": "a" * 63},
                     {"archive_sha256": "0" * 64}, {"source_commit": "0" * 40},
                     {"archive_url": spec["archive_url"].replace("tailrocks", "attacker")},
                     {"archive_url": spec["archive_url"].replace("owned-source", "latest")},
                     {"receipt_url": spec["receipt_url"] + "?query"},
                     {"upstream_base_commit": "e" * 40}, {"license_files": {}},
                     {"license_files": {"LICENSE": "a" * 64, "dir/NOTICE\n": "b" * 64}},
                     {"license_files": {"LICENSE": "a" * 64, "dir\\NOTICE": "b" * 64}},
                     {"license_files": {"LICENSE": "a" * 64, "dir/NOTICÉ": "b" * 64}},
                     {"license_files": {"LICENSE": "a" * 64, ".GiT/NOTICE": "b" * 64}}]
        for mutation in mutations:
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                source.descriptor(json.dumps({**spec, **mutation}), "f" * 40)
        with self.assertRaises(ValueError):
            source.strict_json('{"tool":"mise","tool":"mbx"}')

    def test_extraction_rejects_escape_collisions_and_link_graphs(self):
        file = ("normal", b"hello", 0o644, None)
        cases = [[file, file], [("../bad", b"", 0o644, None)],
                 [(".GIT/config", b"", 0o644, None)],
                 [("Normal", b"", 0o644, None), file],
                 [("bad", b"", 0o777, "../../escape")],
                 [("bad", b"", 0o777, "/escape")],
                 [("a", b"", 0o777, "b"), ("b", b"", 0o777, "a")],
                 [("a", b"", 0o777, "."), ("A/member", b"", 0o644, None)],
                 [("normal", b"", 0o4755, None)]]
        for entries in cases:
            with self.subTest(entries=entries), self.assertRaises(ValueError):
                source.archive_entries(archive(entries))

    def test_extraction_bounds_and_raw_git_tree(self):
        entries = [("Cargo.lock", b"lock", 0o644, None),
                   ("LICENSE", b"license", 0o644, None),
                   ("ignored", b"raw\r\n", 0o755, None),
                   (".gitignore", b"ignored\n", 0o644, None),
                   (".gitattributes", b"ignored text eol=lf filter=evil\n", 0o644, None),
                   ("link", b"", 0o777, "ignored")]
        data = archive(entries)
        with patch.object(source, "MAX_SOURCE", 2), self.assertRaises(ValueError):
            source.archive_entries(data)
        with patch.object(source, "MAX_FILES", 1), self.assertRaises(ValueError):
            source.archive_entries(data)
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp) / "source"
            source.extract(data, directory)
            tree = source.git_tree(directory, {"PATH": os.environ["PATH"], "GIT_CONFIG_COUNT": "999"})
            output = fixture_ignored_blob(self, directory, tree)
            self.assertEqual(output, b"raw\r\n")
            self.assertEqual((directory / "link").readlink(), Path("ignored"))
            self.assertRegex(tree, r"^[a-f0-9]{40}$")

    def test_admission_checks_all_assets_and_final_tree(self):
        spec = fixture()
        data = archive([("Cargo.lock", b"lock", 0o644, None), ("LICENSE", b"license", 0o644, None)])
        with tempfile.TemporaryDirectory() as temp:
            probe = Path(temp) / "probe"
            source.extract(data, probe)
            spec["source_tree"] = source.git_tree(probe, {"PATH": os.environ["PATH"]})
            receipt = {"schema": 1, "status": "STAGED_SOURCE_ONLY", "tool": "mise",
                       "upstream_repository": "https://github.com/jdx/mise",
                       "upstream_base_commit": spec["upstream_base_commit"],
                       "source_commit": spec["source_commit"], "source_tree": spec["source_tree"],
                       "source_archive": {"name": "source.tar", "sha256": spec["archive_sha256"]},
                       "base_patch": {"name": "base.patch", "sha256": spec["patch_sha256"]},
                       "lockfile": {"path": "Cargo.lock", "sha256": spec["lockfile_sha256"]},
                       "license_files": spec["license_files"], "required_hosts": source.HOSTS,
                       "publication": None, "behavioral_qualification": None, "signed_build_provenance": None}
            with patch.object(source, "fetch", side_effect=[json.dumps(receipt), data, b"patch"]) as fetch, \
                    patch.object(source, "verify_patch") as proof:
                source.admit(spec, Path(temp) / "admitted", {"PATH": os.environ["PATH"]})
                self.assertEqual(fetch.call_count, 3)
                proof.assert_called_once()
            with self.assertRaises(ValueError):
                source.validate_receipt(json.dumps({**receipt, "extra": "bad"}), spec)
            with patch.object(source, "fetch", side_effect=[json.dumps(receipt), data, b"patch"]), \
                    patch.object(source, "git_tree", return_value="e" * 40), \
                    patch.object(source, "verify_patch"), self.assertRaises(ValueError):
                source.admit(spec, Path(temp) / "rejected", {"PATH": os.environ["PATH"]})

    def test_fetch_checks_hash_size_and_redirect_transport(self):
        response = unittest.mock.MagicMock()
        response.__enter__.return_value.read.return_value = b"actual source"
        opener = unittest.mock.MagicMock()
        opener.open.return_value = response
        with patch.object(source.urllib.request, "build_opener", return_value=opener):
            self.assertEqual(source.fetch("https://github.com/source", source.digest(b"actual source")), b"actual source")
            with self.assertRaises(ValueError):
                source.fetch("https://github.com/source", "a" * 64)
            with patch.object(source, "MAX_DOWNLOAD", 2), self.assertRaises(ValueError):
                source.fetch("https://github.com/source", source.digest(b"actual source"))
        with self.assertRaises(ValueError):
            source.HttpsRedirect().redirect_request(None, None, 302, "", {}, "http://insecure/source")

    def test_long_raw_paths_allow_only_path_pax_metadata(self):
        name = "source/" + "nested/" * 20 + "file"
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w", format=tarfile.PAX_FORMAT) as tar:
            entry = tarfile.TarInfo(name)
            entry.mode, entry.size = 0o644, 1
            tar.addfile(entry, io.BytesIO(b"a"))
        tar, members = source.archive_entries(output.getvalue())
        self.assertEqual(list(members), [name.removeprefix("source/")])
        tar.close()

    def test_upstream_patch_must_reconstruct_exact_owned_tree(self):
        actual_run = subprocess.run
        with tempfile.TemporaryDirectory() as temporary:
            repository = Path(temporary) / "official-fixture"
            repository.mkdir()
            env = {"PATH": os.environ["PATH"]}
            git = source.git_runner(repository, env)
            (repository / "file").write_bytes(b"base\n")
            git("add", "--all")
            git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.test",
                "commit", "--quiet", "--no-gpg-sign", "-m", "base")
            base = git("rev-parse", "HEAD").decode().strip()
            (repository / "file").write_bytes(b"owned\n")
            valid_patch = git("diff", "--binary", "--full-index", "HEAD")
            git("add", "--all")
            tree = git("write-tree").decode().strip()
            (repository / "file").write_bytes(b"different-owned-source\n")
            wrong_patch = git("diff", "--binary", "--full-index", "HEAD")
            spec = {**fixture(), "source_tree": tree, "upstream_base_commit": base}
            def offline_run(argv, **kwargs):
                if "fetch" in argv:
                    self.assertEqual(argv[-2], "https://github.com/jdx/mise.git")
                    argv = ["git", "-C", argv[argv.index("-C") + 1], "fetch", "--quiet",
                            "--no-tags", str(repository), base]
                return actual_run(argv, **kwargs)
            with patch.object(source, "BASES", {"mise": ("jdx/mise", base)}), \
                    patch.object(source.subprocess, "run", side_effect=offline_run):
                source.verify_patch(spec, valid_patch, Path(temporary) / "valid", env)
                spec["patch_sha256"] = source.digest(wrong_patch)
                with self.assertRaises(ValueError):
                    source.verify_patch(spec, wrong_patch, Path(temporary) / "wrong", env)


class BuildTests(unittest.TestCase):
    def test_command_logs_preserve_summary_diagnostics_and_timing(self):
        argv = ["/verified/mise", *builder.PREFIX, *builder.BUILD["mise"]]
        for returncode in (0, 101):
            result = subprocess.CompletedProcess(argv, returncode, "mbx summary\n", "compiler detail\n")
            stdout, stderr = io.StringIO(), io.StringIO()
            with patch.object(builder.subprocess, "run", return_value=result), \
                    patch.object(builder.time, "monotonic_ns", side_effect=[10, 98]), \
                    redirect_stdout(stdout), redirect_stderr(stderr):
                if returncode:
                    with self.assertRaises(subprocess.CalledProcessError) as failure:
                        builder.run(argv, Path("/source"), {"PATH": "/bin"})
                    self.assertEqual(failure.exception.stdout, "mbx summary\n")
                    self.assertEqual(failure.exception.stderr, "compiler detail\n")
                else:
                    self.assertEqual(builder.run(argv, Path("/source"), {"PATH": "/bin"}), "mbx summary")
            metadata, summary = stdout.getvalue().split("\n", 1)
            self.assertEqual(json.loads(metadata), {"command": argv, "returncode": returncode, "duration_ns": 88})
            self.assertEqual(summary, "mbx summary\n")
            self.assertEqual(stderr.getvalue(), "compiler detail\n")

    def test_launch_failure_records_attempt_and_elapsed_time(self):
        output = io.StringIO()
        with patch.object(builder.subprocess, "run", side_effect=FileNotFoundError("missing bootstrap")), \
                patch.object(builder.time, "monotonic_ns", side_effect=[10, 98]), \
                redirect_stdout(output), self.assertRaises(FileNotFoundError):
            builder.run(["/missing/mise"], Path("/source"), {})
        metadata = json.loads(output.getvalue())
        self.assertEqual(metadata["command"], ["/missing/mise"])
        self.assertIsNone(metadata["returncode"])
        self.assertEqual(metadata["duration_ns"], 88)
        self.assertEqual(metadata["launch_error"], "missing bootstrap")

    def mocked_candidate(self, tool, banner=None, target="aarch64-apple-darwin"):
        spec, calls = fixture(tool), []
        compiler = "rustc 1.99.0\nhost: aarch64-apple-darwin\nrelease: 1.99.0"
        def admit(_spec, path, _env):
            path.mkdir()
            (path / "LICENSE").write_bytes(b"license")
            return b"verified source receipt"
        def run(argv, cwd, env):
            calls.append((argv, env))
            if argv[-2:] == ["rustc", "-vV"]:
                return compiler
            if argv[-2:] == ["cc", "--version"]:
                return "Apple clang 17"
            if "build" in argv:
                binary = Path(env["CARGO_TARGET_DIR"]) / "release" / tool
                binary.parent.mkdir(parents=True)
                binary.write_bytes(NATIVE_BINARY)
                return ""
            return banner or spec["version"] + " macos-arm64"
        with tempfile.TemporaryDirectory() as temporary, patch.object(builder, "admit", admit), \
                patch.object(builder, "run", run), patch.dict(os.environ,
                    {"GH_TOKEN": "secret", "ACTIONS_RUNTIME_TOKEN": "secret", "AWS_SECRET_ACCESS_KEY": "secret",
                     "ImageOS": "macos15", "ImageVersion": "20261003",
                     "OWNED_TOOL_TARGET": target}):
            name, data, receipt, source_receipt = builder.candidate(
                spec, {"mise": Path("/verified/mise"), "mbx": Path("/verified/mbx")}, Path(temporary),
                {"commit": "f" * 40, "run_id": "1", "run_attempt": "1"})
            self.assertEqual(source_receipt, b"verified source receipt")
        return name, data, receipt, calls

    def test_exact_recipe_measured_bytes_and_scrubbed_credentials(self):
        for tool in ("mise", "mbx"):
            name, data, receipt, calls = self.mocked_candidate(tool)
            self.assertIn("aarch64-apple-darwin", name)
            self.assertEqual(receipt["artifact"]["archive_sha256"], source.digest(data))
            self.assertEqual(receipt["artifact"]["binary_sha256"], source.digest(NATIVE_BINARY))
            self.assertEqual(calls[2][0], ["/verified/mise", *builder.PREFIX,
                                          "/verified/mbx", *builder.BUILD[tool][1:]])
            self.assertIsNone(receipt["behavioral_qualification"])
            for _, env in calls:
                self.assertNotIn("GH_TOKEN", env)
                self.assertNotIn("ACTIONS_RUNTIME_TOKEN", env)
                self.assertNotIn("AWS_SECRET_ACCESS_KEY", env)
            with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
                expected = ["mise/bin/mise", "mise/LICENSE"] if tool == "mise" else ["mbx", "LICENSE"]
                self.assertEqual(tar.getnames(), expected)
                self.assertEqual(tar.extractfile(expected[0]).read(), NATIVE_BINARY)

    def test_unowned_debug_or_wrong_compiler_rejected(self):
        for banner in ("2026.10.4", "2026.10.4-owned-cargo-wrapper-DEBUG", "2026.10.5-owned-cargo-wrapper"):
            with self.subTest(banner=banner), self.assertRaises(ValueError):
                self.mocked_candidate("mise", banner)
        with self.assertRaises(ValueError):
            builder.compiler_identity("release: 1.98.0\nhost: aarch64-apple-darwin")
        with self.assertRaises(ValueError):
            builder.compiler_identity("release: 1.99.0\nhost: x86_64-apple-darwin")
        with self.assertRaises(ValueError):
            self.mocked_candidate("mise", target="x86_64-unknown-linux-gnu")

    def test_binary_header_requires_native_executable_architecture(self):
        self.assertEqual(builder.binary_target(NATIVE_BINARY), "aarch64-apple-darwin")
        for machine, host in ((62, "x86_64-unknown-linux-gnu"), (183, "aarch64-unknown-linux-gnu")):
            binary = bytearray(64)
            binary[:7] = b"\x7fELF\x02\x01\x01"
            binary[16:18], binary[18:20] = (3).to_bytes(2, "little"), machine.to_bytes(2, "little")
            self.assertEqual(builder.binary_target(binary), host)
        self.assertIsNone(builder.binary_target(b"#!/bin/sh\necho version"))
        self.assertIsNone(builder.binary_target(NATIVE_BINARY[:12]))

    def test_bootstrap_rejects_unapproved_tuple(self):
        asset = source.official_assets("mise")["aarch64-apple-darwin"]
        values = {"mise": asset, "mbx": asset}
        with patch.dict(os.environ, {"OWNED_TOOL_BUILD_BOOTSTRAP_JSON": json.dumps(values),
                     "OWNED_TOOL_TARGET": "aarch64-apple-darwin"}), \
                patch.object(bootstrap, "fetch") as fetch, self.assertRaises(ValueError):
            bootstrap.bootstrap()
        fetch.assert_not_called()

    def test_bootstrap_closed_shared_contract_and_no_installer_fallback(self):
        target = "aarch64-apple-darwin"
        asset = source.official_assets("mise")[target]
        fake_mbx = {**asset, "version": "fixture", "url": "https://fixture.invalid/mbx"}
        with patch.object(source, "OFFICIAL_MBX_ASSETS", {target: fake_mbx}):
            supplied = {"mise": asset, "mbx": fake_mbx}
            self.assertEqual(source.build_bootstrap_descriptor(json.dumps(supplied), target), supplied)
            for changed in ({"mise": {**asset, "unknown": True}, "mbx": fake_mbx},
                            {"mise": {**asset, "binary_sha256": "a" * 64}, "mbx": fake_mbx},
                            {"mise": asset}, {**supplied, "other": asset}):
                with self.assertRaises(ValueError):
                    source.build_bootstrap_descriptor(json.dumps(changed), target)
        self.assertNotIn("mr-boxington@1.21.1", source.PREFIX)
        self.assertEqual(source.BUILD["mise"][0], "<verified-bootstrap-mbx>")

    def test_missing_official_bootstrap_rows_fail_before_fetch(self):
        target = "aarch64-apple-darwin"
        values = {"mise": source.official_assets("mise")[target], "mbx": source.official_assets("mbx")[target]}
        with patch.dict(os.environ, {"OWNED_TOOL_BUILD_BOOTSTRAP_JSON": json.dumps(values),
                                     "OWNED_TOOL_TARGET": target}), \
                patch.object(source, "OFFICIAL_MBX_ASSETS", {}), \
                patch.object(bootstrap, "fetch") as fetch, self.assertRaises(ValueError):
            bootstrap.bootstrap()
        fetch.assert_not_called()

    def test_bootstrap_installs_only_hash_verified_member(self):
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w:gz") as tar:
            directory = tarfile.TarInfo("mise")
            directory.type = tarfile.DIRTYPE
            tar.addfile(directory)
            file = tarfile.TarInfo("mise/bin/mise")
            file.size = 6
            tar.addfile(file, io.BytesIO(b"binary"))
        self.assertEqual(bootstrap.binary_bytes(output.getvalue(), "tar.gz"), b"binary")
        with self.assertRaises(ValueError):
            bootstrap.binary_bytes(gzip.compress(archive([("bad", b"", 0o777, "outside")])), "tar.gz")


if __name__ == "__main__":
    unittest.main()
