"""Behavioral tests for the producer's cache root proof."""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("tofu_producer_source.sh")
ROOT_KEY = "dir-737461636b732f767063"
TOFU_VERSION = "1.13.1"
PROVIDER = "registry.opentofu.org/hashicorp/random"
PROVIDER_VERSION = "3.6.3"
LOCK = '''provider "registry.opentofu.org/hashicorp/random" {
  version = "3.6.3"
  hashes = ["h1:fixture"]
}
'''
CONFIG = '''terraform {
  required_providers {
    random = {
      source = "registry.opentofu.org/hashicorp/random"
      version = "= 3.6.3"
    }
  }
}
'''


def octal(value):
    return "".join(f"\\{byte:03o}" for byte in value.encode())


def current_host_target():
    system = subprocess.check_output(["/usr/bin/uname", "-s"], text=True).strip()
    machine = subprocess.check_output(["/usr/bin/uname", "-m"], text=True).strip()
    targets = {
        ("Linux", "x86_64"): ("x86_64-unknown-linux-gnu", "linux_amd64"),
        ("Linux", "amd64"): ("x86_64-unknown-linux-gnu", "linux_amd64"),
        ("Linux", "aarch64"): ("aarch64-unknown-linux-gnu", "linux_arm64"),
        ("Linux", "arm64"): ("aarch64-unknown-linux-gnu", "linux_arm64"),
        ("Darwin", "x86_64"): ("x86_64-apple-darwin", "darwin_amd64"),
        ("Darwin", "amd64"): ("x86_64-apple-darwin", "darwin_amd64"),
        ("Darwin", "arm64"): ("aarch64-apple-darwin", "darwin_arm64"),
        ("Darwin", "aarch64"): ("aarch64-apple-darwin", "darwin_arm64"),
    }
    try:
        return targets[(system, machine)]
    except KeyError:
        raise unittest.SkipTest(f"unsupported producer host: {system}:{machine}")


class ProducerRootProofTests(unittest.TestCase):
    def setUp(self):
        self.target, self.platform = current_host_target()
        scratch = Path.cwd() / ".tmp"
        scratch.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(dir=scratch)
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.runner_temp = self.base / "runner"
        self.mise_data = self.base / "mise"
        self.candidate = self.base / "candidate"
        self.output = self.base / "output"
        self.github_output = self.base / "github-output"
        self.hook = self.base / "native-init-reached"
        self.runner_temp.mkdir()
        (self.mise_data / "installs/http-opentofu/1.13.1").mkdir(parents=True)
        self.candidate.mkdir()
        self.write_tofu(fail=True)

    def write_tofu(self, fail):
        tofu = self.mise_data / f"installs/http-opentofu/{TOFU_VERSION}/tofu"
        if fail:
            body = (
                "#!/bin/sh\n"
                f"printf '%s' reached > {shlex.quote(str(self.hook))}\n"
                "exit 1\n"
            )
        else:
            body = (
                "#!/bin/sh\n"
                f"printf '%s' reached > {shlex.quote(str(self.hook))}\n"
                "exit 0\n"
            )
        tofu.write_text(body)
        tofu.chmod(0o700)

    def marker_path(self):
        return self.candidate / ".velnor-root-key"

    def clear_marker(self):
        marker = self.marker_path()
        if marker.exists() or marker.is_symlink():
            marker.unlink()

    def run_producer(self):
        environment = {
            "MISE_DATA_DIR": str(self.mise_data),
            "RUNNER_TEMP": str(self.runner_temp),
            "VELNOR_TOFU_PROVIDER_CANDIDATE": str(self.candidate),
            "VELNOR_TOFU_PROVIDER_OUTPUT": str(self.output),
            "VELNOR_TOFU_PROVIDER_TARGET": self.target,
            "GITHUB_OUTPUT": str(self.github_output),
        }
        self.github_output.touch()
        return subprocess.run(
            [
                "/bin/bash",
                str(SCRIPT),
                self.target,
                octal(LOCK),
                octal(CONFIG),
                TOFU_VERSION,
                ROOT_KEY,
                f"installs/http-opentofu/{TOFU_VERSION}/tofu",
            ],
            check=False,
            capture_output=True,
            env=environment,
        )

    def assert_rejected_before_mise(self, label):
        result = self.run_producer()
        self.assertNotEqual(result.returncode, 0, label)
        self.assertFalse(self.hook.exists(), label)
        self.assertFalse(self.output.exists(), label)

    def test_candidate_marker_requires_exact_regular_single_link(self):
        cases = (
            ("absent", lambda: None),
            ("mismatch", lambda: self.marker_path().write_bytes(b"dir-deadbeef")),
            ("trailing-newline", lambda: self.marker_path().write_bytes(
                (ROOT_KEY + "\n").encode())),
            ("symlink", self.make_symlink_marker),
            ("hardlink", self.make_hardlink_marker),
        )
        for label, make_marker in cases:
            with self.subTest(marker=label):
                self.clear_marker()
                make_marker()
                self.assert_rejected_before_mise(label)

    def make_symlink_marker(self):
        target = self.base / "marker-target"
        target.write_bytes(ROOT_KEY.encode())
        self.marker_path().symlink_to(target)

    def make_hardlink_marker(self):
        target = self.base / "marker-hardlink-target"
        target.write_bytes(ROOT_KEY.encode())
        os.link(target, self.marker_path())

    def test_exact_marker_reaches_native_init(self):
        self.marker_path().write_bytes(ROOT_KEY.encode())
        result = self.run_producer()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.hook.read_text(), "reached")
        self.assertFalse(self.output.exists())

    def test_successful_producer_publishes_exact_marker(self):
        self.write_tofu(fail=False)
        self.marker_path().write_bytes(ROOT_KEY.encode())
        package = self.candidate / PROVIDER / PROVIDER_VERSION / self.platform
        package.mkdir(parents=True)
        (package / "terraform-provider-random_v3.6.3").write_bytes(b"provider")

        result = self.run_producer()

        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(self.hook.read_text(), "reached")
        marker = self.output / ".velnor-root-key"
        self.assertTrue(marker.is_file())
        self.assertFalse(marker.is_symlink())
        self.assertEqual(marker.read_bytes(), ROOT_KEY.encode())
        self.assertEqual(marker.stat().st_nlink, 1)

    def test_bare_install_path_is_not_launched(self):
        bare = self.mise_data / f"installs/opentofu/{TOFU_VERSION}/tofu"
        bare.parent.mkdir(parents=True)
        bare.write_text(
            "#!/bin/sh\n"
            f"printf '%s' bare > {shlex.quote(str(self.hook))}\n"
        )
        bare.chmod(0o700)
        self.assert_rejected_before_mise("bare installation path")


if __name__ == "__main__":
    unittest.main()
