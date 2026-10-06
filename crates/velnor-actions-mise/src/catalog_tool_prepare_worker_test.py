"""Adversarial parser coverage; no installer or target process executes."""
import pathlib
import unittest
import tempfile
import hashlib
import subprocess
from unittest.mock import patch
import catalog_tool_prepare_worker as worker

SHA = "a" * 64
URL = "https://example.test/tool.tar.gz"
SELECTOR = f'http:opentofu[url="{URL}",checksum="sha256:{SHA}",strip_components=0]@1.2.3'
ROOT = "installs/http-opentofu/1.2.3"
CONFIG = "\n".join([
    "velnor-tool-prepare-v1",
    f"tool\t{SELECTOR}\ttofu\t1.2.3",
    f"plan\t{SELECTOR}\t{URL}\t{SHA}\t0\t\t{ROOT}",
    f"launch\t{SELECTOR}\t{ROOT}/tofu\t{SHA}",
])


class ConfigurationTest(unittest.TestCase):
    def reject(self, text, selectors=None):
        with self.assertRaises((ValueError, IndexError)):
            worker.configuration(text, [SELECTOR] if selectors is None else selectors)

    def test_exact_config(self):
        self.assertEqual(worker.configuration(CONFIG, [SELECTOR])[SELECTOR]["plan"], ROOT)

    def test_schema(self):
        self.reject(CONFIG.replace("prepare-v1", "prepare-v2"))

    def test_url_and_checksum_drift(self):
        self.reject(CONFIG.replace(f"\t{URL}\t", "\thttps://foreign.test/tool\t"))
        self.reject(CONFIG.replace(f"\t{SHA}\t0", f"\t{'b' * 64}\t0"))

    def test_missing_plan_or_launch(self):
        self.reject("\n".join(row for row in CONFIG.splitlines() if not row.startswith("plan\t")))
        self.reject("\n".join(row for row in CONFIG.splitlines() if not row.startswith("launch\t")))

    def test_duplicates(self):
        self.reject(CONFIG + "\n" + CONFIG.splitlines()[-1])
        self.reject(CONFIG + "\n" + CONFIG.splitlines()[2])

    def test_path_escape(self):
        self.reject(CONFIG.replace(f"{ROOT}/tofu", "../foreign/tofu"))
        self.reject(CONFIG.replace(f"{ROOT}/tofu", "other/owned/tofu"))

    def test_selector_substitution(self):
        self.reject(CONFIG, [SELECTOR.replace("@1.2.3", "@1.2.4")])

    def test_unknown_and_executable_fields(self):
        self.reject(CONFIG + f"\ncommand\t{SELECTOR}\tsh\t-c")
        self.reject(CONFIG + f"\nenvironment\t{SELECTOR}\tLD_PRELOAD\tlib/evil.so")

    def test_bound(self):
        self.reject(CONFIG + "a" * 65536)

    def test_hash_before_execution(self):
        source = pathlib.Path(worker.__file__).read_text()
        self.assertLess(source.index("    verify_launches(root, tools)"),
                        source.index("        result = subprocess.run"))


class AuthenticatedVariantTest(unittest.TestCase):
    def setUp(self):
        self.sandbox = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.sandbox.name).resolve()
        payload = self.root / ROOT / 'tofu'
        payload.parent.mkdir(parents=True)
        payload.write_bytes(b'qualified-launch-fixture')
        self.payload = payload
        launch_sha = hashlib.sha256(payload.read_bytes()).hexdigest()
        self.config = CONFIG.rsplit(SHA, 1)[0] + launch_sha
        self.argv = ['fixed-worker', str(self.root), '/qualified/mise',
                     self.config.encode().hex(), SELECTOR]

    def tearDown(self):
        self.sandbox.cleanup()

    def test_private_warm_variant_verifies_without_install(self):
        result = subprocess.CompletedProcess([], 0, stdout='OpenTofu v1.2.3')
        with patch.object(worker.sys, 'argv', self.argv), patch.object(
                worker.subprocess, 'run', return_value=result) as run:
            worker.main(install=False)
        self.assertEqual(run.call_count, 1)
        self.assertEqual(run.call_args.args[0][4], 'exec')

    def test_private_warm_corruption_prevents_any_target_call(self):
        self.payload.write_bytes(b'forged exact-version executable')
        with patch.object(worker.sys, 'argv', self.argv), patch.object(
                worker.subprocess, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'launch bytes differ'):
                worker.main(install=False)
        run.assert_not_called()

    def test_ambient_warm_flag_cannot_change_default_cold_worker(self):
        result = subprocess.CompletedProcess([], 0, stdout='OpenTofu v1.2.3')
        with patch.object(worker.sys, 'argv', self.argv), patch.object(
                worker.subprocess, 'run', return_value=result) as run, patch.dict(
                    worker.os.environ, {'VELNOR_AUTHENTICATED': 'true',
                                        'VELNOR_SKIP_INSTALL': 'true'}):
            worker.main()
        self.assertEqual(run.call_count, 2)
        self.assertEqual(run.call_args_list[0].args[0][4], 'install')


if __name__ == "__main__":
    unittest.main()
