"""Qualified Java selection and owned Gradle configuration boundaries."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

import catalog_java_materialize as materialization

BINARY = b"verified catalog Mise"
SELECTOR = "http:graalvm-community-jdk[url=https://fixture.invalid/java.tar.gz]@0.0.1-fixture"
INSTALL_ROOT = "installs/http-graalvm-community-jdk/0.0.1-fixture"
JAVA_BYTES = b"qualified fixture Java"
JAVAC_BYTES = b"qualified fixture javac"


class JavaMaterializationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.temp = Path(self.temporary.name).resolve()
        self.root = self.temp / "velnor/mise"
        self.gradle = self.temp / "velnor/native/gradle"
        self.binary = self.root / "bin/mise"
        self.home = self.root / INSTALL_ROOT
        self.java = self.home / "bin/java"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(BINARY)
        self.binary.chmod(0o700)
        self.java.parent.mkdir(parents=True)
        self.java.write_bytes(JAVA_BYTES)
        self.java.chmod(0o700)
        self.javac = self.java.with_name("javac")
        self.javac.write_bytes(JAVAC_BYTES)
        self.javac.chmod(0o700)
        self.env_file = self.temp / "github-env"
        self.path_file = self.temp / "github-path"
        environment = {
            "RUNNER_TEMP": str(self.temp), "MISE_DATA_DIR": str(self.root),
            "GRADLE_USER_HOME": str(self.gradle), "GITHUB_ENV": str(self.env_file),
            "GITHUB_PATH": str(self.path_file), "JAVA_HOME": "/foreign/java",
            "JAVA_TOOL_OPTIONS": "untrusted", "HTTPS_PROXY": "untrusted",
            "SECRET_TOKEN": "untrusted", "MISE_CONFIG_DIR": "/foreign/config",
        }
        self.environment = patch.dict(os.environ, environment, clear=True)
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.configuration = {
            "domains": {"full": {"mise": "mise", "gradle": "native/gradle"}},
            "binary_sha256": hashlib.sha256(BINARY).hexdigest(),
            "java": {"selector": SELECTOR, "selection_version": "0.0.1-fixture",
                     "reported_version": "fixture-vm", "install_root": INSTALL_ROOT,
                     "home": INSTALL_ROOT, "qualification": "synthetic-java-fixture",
                     "launch": [{"path": INSTALL_ROOT + "/bin/java",
                                 "sha256": hashlib.sha256(JAVA_BYTES).hexdigest()},
                                {"path": INSTALL_ROOT + "/bin/javac",
                                 "sha256": hashlib.sha256(JAVAC_BYTES).hexdigest()}]},
            "flags": ["--no-config", "--no-env", "--no-hooks"],
            "isolation": {"MISE_NO_CONFIG": "1", "MISE_NO_ENV": "1", "MISE_NO_HOOKS": "1",
                          "MISE_LOCKFILE": "0", "MISE_AUTO_INSTALL": "false",
                          "MISE_EXEC_AUTO_INSTALL": "false"},
        }

    def run_materialize(self, stdout=None):
        result = subprocess.CompletedProcess([], 0, stdout=(str(self.home) + "\n").encode()
                                             if stdout is None else stdout, stderr=b"")
        with patch.object(materialization.subprocess, "run", return_value=result) as command:
            materialization.materialize("full", self.configuration)
        return command

    def assert_unpublished(self):
        self.assertFalse((self.gradle / "gradle.properties").exists())
        self.assertFalse(self.env_file.exists())
        self.assertFalse(self.path_file.exists())

    def test_canonical_selector_isolated_environment_and_five_properties(self):
        command = self.run_materialize()
        arguments, options = command.call_args
        self.assertEqual(arguments[0], [str(self.binary), "--no-config", "--no-env", "--no-hooks", "where", SELECTOR])
        self.assertEqual(options["cwd"], str(self.root))
        self.assertEqual(options["stdin"], subprocess.DEVNULL)
        self.assertTrue(options["check"])
        self.assertTrue(options["capture_output"])
        self.assertEqual(options["env"], {
            **self.configuration["isolation"], "HOME": str(self.root / "velnor-java-home"),
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "MISE_DATA_DIR": str(self.root),
            "MISE_CONFIG_DIR": str(self.root / "velnor-empty-config"),
            "MISE_SYSTEM_CONFIG_DIR": str(self.root / "velnor-empty-system-config"),
            "MISE_CEILING_PATHS": str(self.root), "MISE_OFFLINE": "true",
        })
        properties = self.gradle / "gradle.properties"
        self.assertEqual(properties.read_text().splitlines(), [
            "org.gradle.java.home=" + str(self.home),
            "org.gradle.java.installations.auto-detect=false",
            "org.gradle.java.installations.auto-download=false",
            "org.gradle.java.installations.fromEnv=JAVA_HOME",
            "org.gradle.java.installations.paths=" + str(self.home),
        ])
        self.assertEqual(properties.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.env_file.read_text(), "JAVA_HOME=" + str(self.home) + "\n")
        self.assertEqual(self.path_file.read_text(), str(self.java.parent) + "\n")

    def test_bootstrap_domain_uses_its_qualified_mise(self):
        bootstrap = self.temp / "velnor/gradle-source/mise"
        bootstrap.parent.mkdir(parents=True)
        self.root.rename(bootstrap)
        home = bootstrap / INSTALL_ROOT
        self.configuration["domains"]["gradle-bootstrap"] = {
            "mise": "gradle-source/mise", "gradle": "native/gradle"}
        result = subprocess.CompletedProcess([], 0, stdout=(str(home) + "\n").encode(), stderr=b"")
        with patch.dict(os.environ, {"MISE_DATA_DIR": str(bootstrap)}):
            with patch.object(materialization.subprocess, "run", return_value=result) as command:
                materialization.materialize("gradle-bootstrap", self.configuration)
        self.assertEqual(command.call_args.args[0][0], str(bootstrap / "bin/mise"))
        self.assertIn("org.gradle.java.home=" + str(home),
                      (self.gradle / "gradle.properties").read_text())

    def test_selected_home_properties_escape_special_characters(self):
        renamed = self.home.with_name("fixture home:=#!é")
        self.home.rename(renamed)
        self.home = renamed
        relative = str(renamed.relative_to(self.root))
        self.configuration["java"]["install_root"] = relative
        self.configuration["java"]["home"] = relative
        for entry in self.configuration["java"]["launch"]:
            entry["path"] = relative + "/bin/" + Path(entry["path"]).name
        self.run_materialize()
        escaped = materialization.properties_value(str(renamed))
        lines = (self.gradle / "gradle.properties").read_text().splitlines()
        self.assertEqual(lines[0], "org.gradle.java.home=" + escaped)
        self.assertEqual(lines[-1], "org.gradle.java.installations.paths=" + escaped)
        self.assertEqual(self.env_file.read_text(), "JAVA_HOME=" + str(renamed) + "\n")

    def test_invalid_runner_temp_rejected_before_execution(self):
        for invalid in ("relative", str(self.temp) + "/..", str(self.temp) + "//suffix",
                        str(self.temp) + "\n", str(self.temp) + "\x85"):
            with self.subTest(invalid=repr(invalid)), patch.dict(os.environ, {"RUNNER_TEMP": invalid}):
                with patch.object(materialization.subprocess, "run") as command:
                    with self.assertRaisesRegex(ValueError, "temp"):
                        materialization.materialize("full", self.configuration)
                command.assert_not_called()
                self.assert_unpublished()

    def test_wrong_mise_hash_rejected_before_execution(self):
        self.binary.write_bytes(b"wrong binary")
        with patch.object(materialization.subprocess, "run") as command:
            with self.assertRaisesRegex(ValueError, "mise_digest"):
                materialization.materialize("full", self.configuration)
        command.assert_not_called()
        self.assert_unpublished()

    def test_wrong_bindings_rejected_before_execution(self):
        for variable in ("MISE_DATA_DIR", "GRADLE_USER_HOME"):
            with self.subTest(variable=variable), patch.dict(os.environ, {variable: "/foreign"}):
                with patch.object(materialization.subprocess, "run") as command:
                    with self.assertRaisesRegex(ValueError, "binding"):
                        materialization.materialize("full", self.configuration)
                command.assert_not_called()
                self.assert_unpublished()

    def test_malformed_and_foreign_where_outputs_rejected(self):
        outputs = [b"", b"\n", b"relative/path\n", b"/foreign/java\n", b"\xff", b"x" * 4097,
                   (str(self.home) + "\nextra\n").encode(),
                   (str(self.home) + "\r\n").encode(),
                   (str(self.home) + "/../0.0.1-fixture\n").encode(),
                   (str(self.home) + "//\n").encode(),
                   (str(self.root) + "/installs/http-graalvm-community-jdk-escape/version\n").encode()]
        for output in outputs:
            with self.subTest(output=output[:80]):
                with self.assertRaises((ValueError, OSError)):
                    self.run_materialize(output)
                self.assert_unpublished()

    def test_where_failure_never_publishes(self):
        with patch.object(materialization.subprocess, "run", side_effect=subprocess.CalledProcessError(1, [])):
            with self.assertRaises(subprocess.CalledProcessError):
                materialization.materialize("full", self.configuration)
        self.assert_unpublished()

    def test_java_executable_required(self):
        self.java.chmod(0o600)
        with self.assertRaisesRegex(ValueError, "file"):
            self.run_materialize()
        self.assert_unpublished()

    def test_qualified_nested_home_distinct_from_where_install_root(self):
        install = self.home
        nested = install / "Contents/Home"
        nested.mkdir(parents=True)
        (install / "bin").rename(nested / "bin")
        self.configuration["java"]["home"] = INSTALL_ROOT + "/Contents/Home"
        for entry in self.configuration["java"]["launch"]:
            entry["path"] = INSTALL_ROOT + "/Contents/Home/bin/" + Path(entry["path"]).name
        self.run_materialize((str(install) + "\n").encode())
        self.assertEqual(self.env_file.read_text(), "JAVA_HOME=" + str(nested) + "\n")
        self.assertEqual(self.path_file.read_text(), str(nested / "bin") + "\n")
        self.assertEqual((self.gradle / "gradle.properties").read_text().splitlines()[0],
                         "org.gradle.java.home=" + str(nested))

    def test_configured_nested_home_symlink_rejected(self):
        alias = self.home / "qualified-home"
        outside = self.temp / "foreign-java-home"
        outside.mkdir()
        alias.symlink_to(outside, target_is_directory=True)
        self.configuration["java"]["home"] = INSTALL_ROOT + "/qualified-home"
        with self.assertRaises(OSError):
            self.run_materialize()
        self.assert_unpublished()

    def test_all_qualified_launch_hashes_required(self):
        for executable in (self.java, self.javac):
            with self.subTest(executable=executable):
                original = executable.read_bytes()
                executable.write_bytes(b"corrupt qualified launch")
                try:
                    with self.assertRaisesRegex(ValueError, "digest"):
                        self.run_materialize()
                    self.assert_unpublished()
                finally:
                    executable.write_bytes(original)

    def test_same_backend_neighbor_not_selected_by_qualification(self):
        neighbor = self.home.with_name("0.0.2-unqualified")
        (neighbor / "bin").mkdir(parents=True)
        (neighbor / "bin/java").write_bytes(JAVA_BYTES)
        (neighbor / "bin/java").chmod(0o700)
        with self.assertRaisesRegex(ValueError, "foreign_home"):
            self.run_materialize((str(neighbor) + "\n").encode())
        self.assert_unpublished()

    def test_symlink_java_home_rejected(self):
        alias = self.home.parent / "alias"
        alias.symlink_to(self.home, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "foreign_home"):
            self.run_materialize((str(alias) + "\n").encode())
        self.assert_unpublished()

    def test_symlink_binary_and_java_rejected(self):
        for executable in (self.binary, self.java, self.javac):
            with self.subTest(executable=executable):
                original = executable.read_bytes()
                outside = self.temp / "outside-binary"
                outside.write_bytes(original)
                outside.chmod(0o700)
                executable.unlink()
                executable.symlink_to(outside)
                with self.assertRaises(OSError):
                    self.run_materialize()
                self.assert_unpublished()
                executable.unlink()
                executable.write_bytes(original)
                executable.chmod(0o700)

    def test_nonempty_mise_config_rejected_before_execution(self):
        config = self.root / "velnor-empty-config"
        config.mkdir()
        (config / "mise.toml").write_text("untrusted")
        with patch.object(materialization.subprocess, "run") as command:
            with self.assertRaisesRegex(ValueError, "configuration"):
                materialization.materialize("full", self.configuration)
        command.assert_not_called()
        self.assert_unpublished()

    def test_writable_owned_ancestors_rejected(self):
        for ancestor in (self.root / "installs", self.root / "installs/http-graalvm-community-jdk",
                         self.home, self.java.parent, self.binary.parent):
            with self.subTest(ancestor=ancestor):
                original = ancestor.stat().st_mode & 0o777
                ancestor.chmod(0o777)
                try:
                    with self.assertRaisesRegex(ValueError, "owner"):
                        self.run_materialize()
                    self.assert_unpublished()
                finally:
                    ancestor.chmod(original)

    def test_foreign_owned_directory_rejected(self):
        real_fstat = materialization.os.fstat
        root_inode = self.root.stat().st_ino
        def foreign_root(descriptor):
            info = real_fstat(descriptor)
            if info.st_ino == root_inode:
                return Mock(st_uid=os.geteuid() + 1, st_mode=info.st_mode)
            return info
        with patch.object(materialization.os, "fstat", side_effect=foreign_root):
            with self.assertRaisesRegex(ValueError, "owner"):
                self.run_materialize()
        self.assert_unpublished()

    def test_gradle_auto_installed_jdks_rejected(self):
        self.gradle.mkdir(parents=True)
        jdks = self.gradle / "jdks"
        for shape in ("directory", "file", "symlink"):
            with self.subTest(shape=shape):
                if shape == "directory":
                    jdks.mkdir()
                elif shape == "file":
                    jdks.write_text("foreign JDK cache")
                else:
                    jdks.symlink_to(self.home, target_is_directory=True)
                try:
                    with self.assertRaisesRegex(ValueError, "jdk"):
                        self.run_materialize()
                    self.assert_unpublished()
                finally:
                    jdks.rmdir() if shape == "directory" else jdks.unlink()

    def test_properties_symlink_rejected_without_target_mutation(self):
        self.gradle.mkdir(parents=True)
        outside = self.temp / "outside-properties"
        outside.write_text("untouched")
        (self.gradle / "gradle.properties").symlink_to(outside)
        with self.assertRaises(OSError):
            self.run_materialize()
        self.assertEqual(outside.read_text(), "untouched")
        self.assertFalse(self.env_file.exists())

    def test_properties_replacement_is_atomic_and_failure_preserves_original(self):
        self.gradle.mkdir(parents=True)
        properties = self.gradle / "gradle.properties"
        properties.write_text("original")
        replace = materialization.os.replace
        def observe_replace(source, destination, **options):
            self.assertEqual(properties.read_text(), "original")
            self.assertTrue(source.startswith(".velnor-java-"))
            self.assertEqual(destination, "gradle.properties")
            return replace(source, destination, **options)
        with patch.object(materialization.os, "replace", side_effect=observe_replace) as atomic:
            self.run_materialize()
        atomic.assert_called_once()
        properties.write_text("preserved")
        with patch.object(materialization.os, "replace", side_effect=OSError("replace rejected")):
            with self.assertRaisesRegex(OSError, "replace rejected"):
                self.run_materialize()
        self.assertEqual(properties.read_text(), "preserved")
        self.assertEqual(list(self.gradle.iterdir()), [properties])

    def test_properties_encoding_keeps_one_line_and_escapes_unicode(self):
        self.assertEqual(materialization.properties_value("/a b:=#!\\é"),
                         "/a\\ b\\:\\=\\#\\!\\\\\\u00e9")
        for invalid in ("/a,b", "/a\n", "/a\r", "/a\x00", "/a\x85"):
            with self.subTest(invalid=repr(invalid)):
                with self.assertRaisesRegex(ValueError, "home_encoding"):
                    materialization.properties_value(invalid)


if __name__ == "__main__":
    unittest.main()
