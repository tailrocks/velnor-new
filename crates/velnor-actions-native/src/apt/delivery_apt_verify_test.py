"""Adversarial checks for the generated native verifier."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import struct
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import delivery_apt_verify as adapter
import delivery_apt_core as core
from delivery_apt_core import loads, read_json


class VerifyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.previous = Path.cwd()
        os.chdir(self.directory.name)
        self.config = {'source_repository': 'owner/source', 'package': 'example',
                       'binary': 'example', 'identity_directory': 'example',
                       'manifest_schema': 'example.release/v1', 'keyring': 'example.gpg',
                       'signer_fingerprint': 'A' * 40, 'signer_workflow': '.github/workflows/release.yml',
                       'oci_image_repository': 'ghcr.io/owner/example',
                       'oci_signer_workflow': '.github/workflows/oci.yml'}
        Path('example.gpg').write_bytes(b'key')
        self.commit = 'a' * 40
        self.version = '1.2.3~preview.41+aaaaaaa'

    def tearDown(self):
        os.chdir(self.previous)
        self.directory.cleanup()

    def elf(self, arch):
        binary = bytearray(64)
        binary[:7] = b'\x7fELF\x02\x01\x01'
        struct.pack_into('<HHI', binary, 16, 2, {'amd64': 62, 'arm64': 183}[arch], 1)
        struct.pack_into('<H', binary, 52, 64)
        return bytes(binary)

    def tar(self, unsafe=False, arch='amd64'):
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode='w') as archive:
            contents = {'usr/bin/example': self.elf(arch),
                        'usr/share/example/build-identity.json': json.dumps(
                            {'source_sha': self.commit, 'crate_version': '1.2.3'}).encode()}
            for name, payload in contents.items():
                item = tarfile.TarInfo(name)
                item.size = len(payload)
                archive.addfile(item, io.BytesIO(payload))
            if unsafe:
                item = tarfile.TarInfo('usr/share/example/escape')
                item.type, item.linkname = tarfile.SYMTYPE, '/etc/passwd'
                archive.addfile(item)
        return output.getvalue()

    def execute(self, unsafe=False, corrupt=False, swapped=False):
        names = ['example-preview-1.2.3.preview.41+aaaaaaa-' + arch + '.deb' for arch in adapter.ARCHES]
        sha = hashlib.sha256(b'deb').hexdigest()
        manifest = {'schema': 'example.release/v1', 'source_repository': 'owner/source',
                    'source_ref': 'refs/heads/main', 'source_commit': self.commit,
                    'version': self.version, 'assets': [{'name': name, 'sha256': sha} for name in names]}
        if corrupt:
            manifest['source_ref'] = 'refs/heads/evil'
        calls = []

        def run(argv):
            calls.append(argv)
            if argv[0] == 'gpg':
                return 'pub:::::::::\nfpr:::::::::' + 'A' * 40 + ':\n'
            if argv[:3] == ['gh', 'release', 'download']:
                for index, token in enumerate(argv):
                    if token != '--pattern':
                        continue
                    name = argv[index + 1]
                    payload = b'deb'
                    if name == 'release-manifest.json':
                        payload = json.dumps(manifest).encode()
                    elif name == 'SHA256SUMS':
                        payload = ''.join(sha + '  ' + name + '\n' for name in names).encode()
                    elif name.endswith('.sha256'):
                        payload = (sha + '  ' + name[:-7] + '\n').encode()
                    (Path('incoming') / name).write_bytes(payload)
            if argv[0] == 'dpkg-deb':
                return {'Package': 'example', 'Version': self.version,
                        'Architecture': 'arm64' if 'arm64' in argv[2] else 'amd64'}[argv[3]]
            return ''

        def payload(argv, **kwargs):
            arch = 'arm64' if 'arm64' in argv[2] else 'amd64'
            if swapped:
                arch = 'amd64' if arch == 'arm64' else 'arm64'
            return subprocess.CompletedProcess(argv, 0, stdout=self.tar(unsafe, arch))
        with patch.dict(os.environ, {'CHANNEL': 'preview', 'INPUT_VERSION': '', 'INPUT_COMMIT': '',
                                    'GITHUB_SHA': 'b' * 40}), \
                patch.object(adapter, 'run', run), patch.object(core.subprocess, 'run', side_effect=payload):
            marker = adapter.verify(self.config)
        return marker, calls

    def test_preview_discovery_and_attestation_binding(self):
        marker, calls = self.execute()
        self.assertEqual(marker['version'], self.version)
        self.assertEqual(marker['commit'], self.commit)
        self.assertEqual(marker['consumer_source_sha'], 'b' * 40)
        self.assertNotEqual(marker['consumer_source_sha'], marker['commit'])
        self.assertEqual(read_json('incoming/.apt-verified.json'), marker)
        attestations = [argv for argv in calls if argv[:3] == ['gh', 'attestation', 'verify']]
        self.assertEqual(len(attestations), 2)
        for argv in attestations:
            self.assertIn('--deny-self-hosted-runners', argv)
            self.assertEqual(argv[argv.index('--source-digest') + 1], self.commit)
            self.assertEqual(argv[argv.index('--source-ref') + 1], 'refs/heads/main')

    def test_unsafe_deb_never_arms_marker(self):
        with self.assertRaisesRegex(ValueError, 'links'):
            self.execute(unsafe=True)
        self.assertFalse(Path('incoming/.apt-verified.json').exists())

    def test_wrong_preview_ref_never_arms_marker(self):
        with self.assertRaisesRegex(ValueError, 'source_ref'):
            self.execute(corrupt=True)
        self.assertFalse(Path('incoming/.apt-verified.json').exists())

    def test_duplicate_json_rejected(self):
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            loads('{"schema":"good", "schema":"bad"}')

    def test_traversal_config_rejected(self):
        self.config['keyring'] = '../escape.gpg'
        with self.assertRaisesRegex(ValueError, 'keyring'):
            adapter.validate_config(self.config)

    def test_swapped_deb_binary_never_arms_marker(self):
        with self.assertRaisesRegex(ValueError, 'architecture'):
            self.execute(swapped=True)
        self.assertFalse(Path('incoming/.apt-verified.json').exists())

    def test_elf_swapped_architecture_rejected(self):
        for source, target in [('amd64', 'arm64'), ('arm64', 'amd64')]:
            with self.assertRaisesRegex(ValueError, 'architecture'):
                core.elf_identity(self.elf(source), target)

    def test_elf_executable_header_required(self):
        for offset, value in [(4, 1), (5, 2), (6, 0), (16, 1), (20, 0), (52, 63)]:
            binary = bytearray(self.elf('amd64'))
            binary[offset] = value
            with self.assertRaises(ValueError):
                core.elf_identity(bytes(binary), 'amd64')
        with self.assertRaises(ValueError):
            core.elf_identity(b'NEVER EXECUTE', 'amd64')

    def test_debian_package_lexical_parity(self):
        for package in ['ab', 'libexample+addon', 'a.b-c', '01']:
            self.config['package'] = package
            adapter.validate_config(self.config)
        for package in ['a', 'Example', 'example_package', '-example']:
            self.config['package'] = package
            with self.assertRaisesRegex(ValueError, 'package'):
                adapter.validate_config(self.config)

    def oci_record(self):
        sha, manifest = 'sha256:' + 'b' * 64, 'c' * 64
        labels = {'version': '1.2.3', 'revision': self.commit,
                  'source': 'https://github.com/owner/source', 'manifest_sha256': manifest}
        return {'oci_index_digest': sha, 'oci_image_ref': 'ghcr.io/owner/example@' + sha,
                'oci_labels': labels}, manifest

    def test_untrusted_oci_repository_rejected_before_commands(self):
        record, manifest = self.oci_record()
        record['oci_image_ref'] = record['oci_image_ref'].replace('owner/example', 'attacker/example')
        with patch.object(adapter, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'trusted policy'):
                adapter.oci_verify(self.config, record, {}, '1.2.3', self.commit, manifest)
            run.assert_not_called()

    def test_oci_attestation_binds_digest_source_and_workflow(self):
        record, manifest = self.oci_record()
        responses = ['', json.dumps({'manifest': {'digest': record['oci_index_digest'], 'manifests': []}})]
        with patch.object(adapter, 'run', side_effect=responses) as run:
            adapter.oci_verify(self.config, record, {}, '1.2.3', self.commit, manifest)
            argv = run.call_args_list[0].args[0]
            self.assertEqual(argv[:4], ['gh', 'attestation', 'verify', 'oci://' + record['oci_image_ref']])
            for key, value in [('--repo', 'owner/source'), ('--source-digest', self.commit),
                               ('--source-ref', 'refs/tags/v1.2.3'),
                               ('--signer-workflow', 'owner/source/.github/workflows/oci.yml')]:
                self.assertEqual(argv[argv.index(key) + 1], value)
            self.assertIn('--deny-self-hosted-runners', argv)
            self.assertEqual(run.call_args_list[1].args[0][0], 'docker')

    def test_failed_oci_attestation_stops_before_inspection(self):
        record, manifest = self.oci_record()
        with patch.object(adapter, 'run', side_effect=subprocess.CalledProcessError(1, ['gh'])) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                adapter.oci_verify(self.config, record, {}, '1.2.3', self.commit, manifest)
            self.assertEqual(run.call_count, 1)

    def test_oci_policy_lexical_validation(self):
        for image in ['evil.io/owner/image', 'ghcr.io/owner/image:latest', 'ghcr.io/owner/../image',
                      'ghcr.io/owner/image@sha256:abc', 'ghcr.io/Owner/image']:
            self.config['oci_image_repository'] = image
            with self.assertRaisesRegex(ValueError, 'OCI image repository'):
                adapter.validate_config(self.config)

    def test_architecture_target_binding(self):
        record, unused = self.oci_record()
        record['architectures'] = [
            {'arch': 'amd64', 'target': 'x86_64-unknown-linux-gnu'},
            {'arch': 'arm64', 'target': 'aarch64-unknown-linux-gnu'}]
        self.assertEqual(set(adapter.architecture_rows(record)), {'amd64', 'arm64'})
        record['architectures'][0]['target'] = 'aarch64-unknown-linux-gnu'
        with self.assertRaisesRegex(ValueError, 'target mismatch'):
            adapter.architecture_rows(record)
        record['architectures'][0].pop('target')
        with self.assertRaisesRegex(ValueError, 'target mismatch'):
            adapter.architecture_rows(record)


if __name__ == '__main__':
    unittest.main()
