"""Real GPG regression: publication keyrings contain only the pinned primary."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from delivery_apt_core import config_digest, write_json
from delivery_apt_stage import guard, inventory, stage
from delivery_apt_stage_feed import verify_signature
from delivery_apt_stage_publish import publish
from delivery_apt_verify import signer


@unittest.skipUnless(shutil.which('gpg') and shutil.which('gpgv'), 'GPG unavailable')
class KeyringMutationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.keys = tempfile.TemporaryDirectory(prefix='velnor-keyring-test-')
        cls.home = Path(cls.keys.name) / 'gpg'
        cls.home.mkdir(mode=0o700)
        cls.command = ['gpg', '--batch', '--homedir', str(cls.home), '--pinentry-mode',
                       'loopback', '--passphrase', '']
        cls.pinned = cls.make_key('Pinned fixture <pinned@example.test>')
        cls.gpg(['--quick-add-key', cls.pinned, 'ed25519', 'sign', '0'])
        cls.trusted = cls.gpg(['--export', cls.pinned])
        cls.rogue = cls.gpg(['--export', cls.make_key('Rogue fixture <rogue@example.test>')])

    @classmethod
    def tearDownClass(cls):
        subprocess.run(['gpgconf', '--homedir', str(cls.home), '--kill', 'gpg-agent'],
                       check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        cls.keys.cleanup()

    @classmethod
    def gpg(cls, arguments):
        return subprocess.run(cls.command + arguments, check=True, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE).stdout

    @classmethod
    def make_key(cls, identity):
        cls.gpg(['--quick-generate-key', identity, 'ed25519', 'cert', '0'])
        listing = cls.gpg(['--with-colons', '--list-keys', identity]).decode()
        return next(line.split(':')[9] for line in listing.splitlines() if line.startswith('fpr:'))

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='velnor-keyring-case-')
        self.previous = Path.cwd()
        os.chdir(self.temporary.name)
        self.environment = patch.dict(os.environ, {'GNUPGHOME': str(self.home),
                                      'GITHUB_RUN_ID': '42', 'GITHUB_RUN_ATTEMPT': '1',
                                      'GITHUB_SHA': 'a' * 40, 'INPUT_SUITE': 'stable'})
        self.environment.start()
        Path('keys').mkdir()
        Path('keys/publisher.gpg').write_bytes(self.trusted)
        self.config = {'keyring': 'keys/publisher.gpg', 'signer_fingerprint': self.pinned}

    def tearDown(self):
        self.environment.stop()
        os.chdir(self.previous)
        self.temporary.cleanup()

    def test_primary_and_signing_subkey_accepted(self):
        signer(self.config)
        document = Path('record')
        document.write_bytes(b'authenticated fixture\n')
        signature = Path('record.sig')
        self.gpg(['--local-user', self.pinned, '--output', str(signature),
                  '--detach-sign', str(document)])
        verify_signature(self.config, document.read_bytes(), signature.read_bytes())

    def test_appended_rogue_primary_refused_by_signer(self):
        Path(self.config['keyring']).write_bytes(self.trusted + self.rogue)
        with self.assertRaisesRegex(ValueError, 'keyring signer'):
            signer(self.config)

    def test_stage_refuses_mutated_source_before_fetch_or_signing(self):
        Path(self.config['keyring']).write_bytes(self.trusted + self.rogue)
        with patch('delivery_apt_stage.validate_config'):
            with patch('delivery_apt_stage.verified_input') as verified:
                with patch('delivery_apt_stage.live_suite') as network:
                    with patch('delivery_apt_stage.publish') as publication:
                        with self.assertRaisesRegex(ValueError, 'keyring signer'):
                            stage(self.config)
        verified.assert_not_called()
        network.assert_not_called()
        publication.assert_not_called()

    def test_guard_refuses_mutated_copy_with_coherent_inventory(self):
        root = Path('public')
        root.mkdir()
        (root / 'publisher.gpg').write_bytes(self.trusted + self.rogue)
        proof = {'schema': 'velnor.apt-stage/v1', 'config_sha256': config_digest(self.config),
                 'workflow_run_id': '42', 'workflow_run_attempt': '1',
                 'consumer_source_sha': 'a' * 40, 'files': inventory(root)}
        write_json(root / '.apt-stage.json', proof)
        signer(self.config)
        with patch('delivery_apt_stage.validate_config'):
            with patch('delivery_apt_stage.local_suite') as local:
                with patch('delivery_apt_stage.live_suite') as network:
                    with self.assertRaisesRegex(ValueError, 'keyring signer'):
                        guard(self.config)
        local.assert_not_called()
        network.assert_not_called()
        self.assertEqual(Path(self.config['keyring']).read_bytes(), self.trusted)

    def test_publish_refuses_copy_mutation_before_package_or_private_key_use(self):
        original_copy = shutil.copyfile
        def changed_copy(source, destination):
            result = original_copy(source, destination)
            with Path(destination).open('ab') as output:
                output.write(self.rogue)
            return result
        with patch('delivery_apt_stage_publish.rollback', return_value=(None, None)):
            with patch('delivery_apt_stage_publish.shutil.copyfile', side_effect=changed_copy):
                with patch('delivery_apt_stage_publish.stage_pool') as pool:
                    with patch('delivery_apt_stage_publish.signing_key') as private_key:
                        with self.assertRaisesRegex(ValueError, 'keyring signer'):
                            publish(self.config, {}, None, None)
        pool.assert_not_called()
        private_key.assert_not_called()
        self.assertEqual(Path(self.config['keyring']).read_bytes(), self.trusted)


if __name__ == '__main__':
    unittest.main()
