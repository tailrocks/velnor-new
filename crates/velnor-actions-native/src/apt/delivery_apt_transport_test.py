"""Immutable APT transport identity, digest and hostile ZIP regression tests."""
import hashlib
import io
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import Mock, patch
import zipfile

import delivery_apt_transport as transport


ENVIRONMENT = {'GITHUB_REPOSITORY': 'example/feed', 'GITHUB_SHA': 'a' * 40,
               'GITHUB_RUN_ID': '42', 'GITHUB_RUN_ATTEMPT': '2', 'ARTIFACT_ID': '7',
               'ARTIFACT_DIGEST': 'b' * 64, 'GH_TOKEN': 'read-token', 'GH_HOST': 'github.com',
               'GITHUB_SERVER_URL': 'https://github.com', 'GITHUB_API_URL': 'https://api.github.com'}


def archive(entries=None):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, 'w') as output:
        for name, content in entries or [('manifest.json', b'original'), ('.apt-verified.json', b'proof')]:
            output.writestr(name, content)
    return stream.getvalue()


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.previous = Path.cwd()
        os.chdir(self.temporary.name)
        self.environment = patch.dict(os.environ, ENVIRONMENT)
        self.environment.start()
        self.ctx = transport.context('incoming')
        self.run = {'id': 42, 'run_attempt': 2, 'head_sha': 'a' * 40,
                    'repository': {'id': 9, 'full_name': 'example/feed'}}
        self.job = {'name': 'Verify apt feed', 'run_id': 42, 'run_attempt': 2,
                    'head_sha': 'a' * 40, 'status': 'completed', 'conclusion': 'success'}
        self.artifact = {'id': 7, 'name': 'apt-incoming-42-2', 'expired': False,
                         'size_in_bytes': 123, 'digest': 'sha256:' + 'b' * 64,
                         'workflow_run': {'id': 42, 'repository_id': 9,
                                          'head_repository_id': 9, 'head_sha': 'a' * 40}}

    def tearDown(self):
        self.environment.stop()
        os.chdir(self.previous)
        self.temporary.cleanup()

    def test_api_binary_bound_and_fixed_environment(self):
        process = Mock()
        process.stdout = io.BytesIO(b'too large')
        process.poll.return_value = None
        with patch.object(transport.subprocess, 'Popen', return_value=process) as launch:
            with patch.object(transport.threading, 'Timer'):
                with self.assertRaisesRegex(ValueError, 'size bound'):
                    transport.api('repos/example/feed/actions/artifacts/7/zip', 2)
        process.kill.assert_called_once()
        command = launch.call_args.args[0]
        self.assertIn('github.com', command)
        self.assertEqual(command[-1], 'repos/example/feed/actions/artifacts/7/zip')
        environment = launch.call_args.kwargs['env']
        self.assertEqual(environment['GH_HOST'], 'github.com')
        self.assertEqual(set(environment), {'PATH', 'GH_TOKEN', 'GH_HOST', 'GH_CONFIG_DIR',
                                          'GH_PROMPT_DISABLED'})

    def test_exact_metadata_receipt_accepted(self):
        self.assertEqual(transport.verify_run(self.ctx, self.run), 9)
        self.assertEqual(transport.verify_artifact(self.ctx, self.artifact, 9), 123)
        with patch.object(transport, 'document', return_value={'jobs': [self.job]}):
            transport.verify_producer(self.ctx)

    def test_qualified_action_and_rest_digest_shapes(self):
        digest = 'b' * 64
        self.assertEqual(transport.checksum(digest), digest)
        self.assertEqual(transport.checksum('sha256:' + digest, rest=True), digest)
        for value in ('sha256:' + digest, 'SHA256:' + digest, digest.upper(),
                      digest + '\n', digest[:-1], 'sha256:sha256:' + digest):
            with self.subTest(source='action', value=value), self.assertRaises(ValueError):
                transport.checksum(value)
        for value in (digest, 'SHA256:' + digest, 'sha256:' + digest.upper(),
                      'sha256:' + digest + '\n', 'sha256:sha256:' + digest):
            with self.subTest(source='rest', value=value), self.assertRaises(ValueError):
                transport.checksum(value, rest=True)

    def test_run_identity_mutations_refused(self):
        for key, value in [('id', 43), ('run_attempt', 1), ('head_sha', 'c' * 40),
                           ('repository', {'id': 10, 'full_name': 'evil/feed'})]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                transport.verify_run(self.ctx, dict(self.run, **{key: value}))

    def test_artifact_mutations_refused(self):
        mutations = [('id', 8), ('name', 'apt-incoming-42-1'), ('expired', True),
                     ('digest', 'sha256:' + 'c' * 64), ('size_in_bytes', transport.MAX_ARCHIVE + 1)]
        for key, value in mutations:
            with self.subTest(key=key), self.assertRaises(ValueError):
                transport.verify_artifact(self.ctx, dict(self.artifact, **{key: value}), 9)
        for key, value in [('id', 43), ('repository_id', 10), ('head_repository_id', 10),
                           ('head_sha', 'c' * 40), ('run_attempt', 1)]:
            artifact = dict(self.artifact, workflow_run=dict(self.artifact['workflow_run'], **{key: value}))
            with self.subTest(key=key), self.assertRaises(ValueError):
                transport.verify_artifact(self.ctx, artifact, 9)

    def test_producer_identity_and_success_required(self):
        for key, value in [('name', 'Untrusted producer'), ('run_id', 43), ('run_attempt', 1),
                           ('head_sha', 'c' * 40), ('status', 'in_progress'), ('conclusion', 'failure')]:
            with patch.object(transport, 'document', return_value={'jobs': [dict(self.job, **{key: value})]}):
                with self.subTest(key=key), self.assertRaises(ValueError):
                    transport.verify_producer(self.ctx)
        for jobs in ([], [self.job, self.job]):
            with patch.object(transport, 'document', return_value={'jobs': jobs}), self.assertRaises(ValueError):
                transport.verify_producer(self.ctx)

    def test_fixed_context_rejects_host_and_env_injection(self):
        for variable, value in [('GITHUB_REPOSITORY', '../evil'), ('GITHUB_RUN_ID', '42?foo=1'),
                                ('ARTIFACT_ID', '../7'), ('ARTIFACT_DIGEST', 'SHA256:' + 'b' * 64),
                                ('GH_HOST', 'evil.test'), ('GITHUB_API_URL', 'https://evil.test')]:
            with patch.dict(os.environ, {variable: value}), self.subTest(variable=variable):
                with self.assertRaises(ValueError):
                    transport.context('incoming')

    def test_atomic_extraction_preserves_bytes(self):
        transport.extract(archive(), 'incoming')
        self.assertEqual(Path('incoming/manifest.json').read_bytes(), b'original')
        with self.assertRaisesRegex(ValueError, 'destination exists'):
            transport.extract(archive(), 'incoming')

    def test_hostile_zip_paths_refused_without_output(self):
        cases = ['../evil', '/evil', 'a/../../evil', 'a\\evil', 'a//evil', './evil',
                 'C:evil', 'a/./evil']
        for name in cases:
            with self.subTest(name=name), self.assertRaises(ValueError):
                transport.extract(archive([(name, b'bad')]), 'incoming')
            self.assertFalse(Path('incoming').exists())

    def test_null_zip_name_refused(self):
        payload = archive([('aXevil', b'bad')]).replace(b'aXevil', b'a\x00evil')
        with self.assertRaisesRegex(ValueError, 'unsafe ZIP path'):
            transport.extract(payload, 'incoming')
        self.assertFalse(Path('incoming').exists())

    def test_case_duplicates_and_file_directory_collisions_refused(self):
        cases = [[('file', b'1'), ('file', b'2')], [('file', b'1'), ('FILE', b'2')],
                 [('a/x', b'1'), ('A/y', b'2')], [('a', b'1'), ('a/b', b'2')],
                 [('a/b', b'1'), ('a', b'2')]]
        for entries in cases:
            with self.subTest(entries=entries), self.assertRaises(ValueError):
                transport.extract(archive(entries), 'incoming')
            self.assertFalse(Path('incoming').exists())

    def test_symlink_and_special_zip_entries_refused(self):
        for mode in (stat.S_IFLNK, stat.S_IFIFO, stat.S_IFCHR, stat.S_IFSOCK):
            entry = zipfile.ZipInfo('evil')
            entry.create_system = 3
            entry.external_attr = (mode | 0o777) << 16
            with self.subTest(mode=mode), self.assertRaisesRegex(ValueError, 'special'):
                transport.extract(archive([(entry, b'target')]), 'incoming')

    def test_zip_size_and_count_bounds_enforced(self):
        for setting, value in [('MAX_FILE', 1), ('MAX_CONTENT', 1), ('MAX_ENTRIES', 1)]:
            with patch.object(transport, setting, value), self.assertRaises(ValueError):
                transport.extract(archive(), 'incoming')
            self.assertFalse(Path('incoming').exists())

    def download_fixture(self, payload):
        digest = hashlib.sha256(payload).hexdigest()
        os.environ['ARTIFACT_DIGEST'] = digest
        self.artifact.update(digest='sha256:' + digest, size_in_bytes=len(payload))
        return [self.run, {'jobs': [self.job]}, self.artifact, self.run]

    def test_archive_digest_checked_before_extraction(self):
        payload = archive()
        documents = self.download_fixture(payload)
        with patch.object(transport, 'document', side_effect=documents):
            with patch.object(transport, 'api', return_value=b'x' * len(payload)):
                with self.assertRaisesRegex(ValueError, 'archive digest mismatch'):
                    transport.download('incoming')
        self.assertFalse(Path('incoming').exists())

    def test_rerun_during_transfer_refused(self):
        payload = archive()
        documents = self.download_fixture(payload)
        documents[-1] = dict(self.run, run_attempt=3)
        with patch.object(transport, 'document', side_effect=documents):
            with patch.object(transport, 'api', return_value=payload):
                with self.assertRaisesRegex(ValueError, 'attempt mismatch'):
                    transport.download('incoming')
        self.assertFalse(Path('incoming').exists())

    def test_download_only_fixed_immutable_routes(self):
        payload = archive()
        with patch.object(transport, 'document', side_effect=self.download_fixture(payload)) as metadata:
            with patch.object(transport, 'api', return_value=payload) as binary:
                transport.download('incoming')
        binary.assert_called_once_with('repos/example/feed/actions/artifacts/7/zip', transport.MAX_ARCHIVE)
        self.assertEqual(metadata.call_count, 4)
        self.assertEqual(Path('incoming/manifest.json').read_bytes(), b'original')


if __name__ == '__main__':
    unittest.main()
