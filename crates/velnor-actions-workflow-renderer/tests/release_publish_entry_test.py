# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


path = Path(__file__).resolve().parents[1] / 'src/release_publish_entry.py'
source = path.read_text().replace('__VELNOR_JOB_TIMEOUT_SECONDS__', '1800')
entry = {'__name__': 'publisher_entry_fixture'}
exec(compile(source, str(path), 'exec'), entry)


def context(mode):
    result = {
        'GITHUB_REPOSITORY': 'example/project', 'GITHUB_REPOSITORY_ID': '7',
        'GITHUB_REPOSITORY_OWNER_ID': '8', 'GITHUB_RUN_ID': '9', 'GITHUB_RUN_ATTEMPT': '1',
        'GITHUB_JOB': 'release-publish' if mode == 'trusted-publishing' else 'release-publish-bootstrap',
        'GITHUB_SHA': 'a' * 40, 'GITHUB_REF': 'refs/heads/main', 'GITHUB_EVENT_NAME': 'push',
        'GITHUB_WORKFLOW_REF': 'example/project/.github/workflows/release.yml@refs/heads/main',
        'GITHUB_WORKFLOW_SHA': 'a' * 40, 'GITHUB_SERVER_URL': 'https://github.com',
        'GITHUB_API_URL': 'https://api.github.com', 'RUNNER_ENVIRONMENT': 'github-hosted',
        'RUNNER_OS': 'Linux', 'RUNNER_ARCH': 'X64', 'VELNOR_PREFLIGHT_ELIGIBLE': 'true',
        'VELNOR_PREFLIGHT_SOURCE_SHA': 'b' * 40, 'VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID': '10',
        'VELNOR_PREFLIGHT_RUST_ARTIFACT_ID': '11', 'GH_TOKEN': 'fixture-gh',
    }
    if mode == 'trusted-publishing':
        result.update({'ACTIONS_ID_TOKEN_REQUEST_TOKEN': 'fixture-oidc',
                       'ACTIONS_ID_TOKEN_REQUEST_URL': 'https://example.actions.githubusercontent.com/token'})
    else:
        result['CARGO_REGISTRY_TOKEN'] = 'fixture-registry'
    return result


class PublisherEntryTest(unittest.TestCase):
    def test_context_accepts_only_matching_release_modes(self):
        for mode in ('trusted-publishing', 'bootstrap'):
            values = context(mode)
            values.update({'LD_PRELOAD': 'hostile', 'HTTPS_PROXY': 'hostile', 'GITHUB_OUTPUT': 'hostile'})
            with self.subTest(mode=mode), patch.dict(os.environ, values, clear=True):
                accepted = entry['read_context'](mode)
                self.assertNotIn('LD_PRELOAD', accepted)
                self.assertNotIn('HTTPS_PROXY', accepted)
                self.assertNotIn('GITHUB_OUTPUT', accepted)
                self.assertEqual(accepted['GITHUB_JOB'], values['GITHUB_JOB'])

    def test_context_rejects_unapproved_identity_and_credentials(self):
        for key, value in (
            ('GITHUB_JOB', 'release-publish-bootstrap'), ('GITHUB_RUN_ATTEMPT', '2'),
            ('GITHUB_EVENT_NAME', 'pull_request'), ('GITHUB_REF', 'refs/tags/v1'),
            ('GITHUB_WORKFLOW_REF', 'attacker/project/.github/workflows/release.yml@refs/heads/main'),
            ('GITHUB_REPOSITORY_ID', '0'), ('VELNOR_PREFLIGHT_SOURCE_SHA', 'nope'),
            ('VELNOR_PREFLIGHT_ELIGIBLE', 'false'), ('VELNOR_PREFLIGHT_RUST_ARTIFACT_ID', '0'),
            ('VELNOR_PREFLIGHT_RUST_ARTIFACT_ID', '10'), ('GITHUB_RUN_ID', '18446744073709551616'),
            ('ACTIONS_ID_TOKEN_REQUEST_URL', 'https://example.actions.githubusercontent.com.attacker.test/token'),
            ('ACTIONS_ID_TOKEN_REQUEST_TOKEN', ''), ('RUNNER_ENVIRONMENT', 'self-hosted'),
        ):
            values = context('trusted-publishing')
            values[key] = value
            with self.subTest(key=key), patch.dict(os.environ, values, clear=True):
                with self.assertRaises(ValueError):
                    entry['read_context']('trusted-publishing')

    def test_context_rejects_invalid_branch_even_when_workflow_ref_matches(self):
        for branch in ('refs/heads/a..b', 'refs/heads/a.lock', 'refs/heads/a//b', 'refs/heads/a b'):
            values = context('trusted-publishing')
            values['GITHUB_REF'] = branch
            values['GITHUB_WORKFLOW_REF'] = 'example/project/.github/workflows/release.yml@' + branch
            with self.subTest(branch=branch), patch.dict(os.environ, values, clear=True):
                with self.assertRaises(ValueError):
                    entry['read_context']('trusted-publishing')

    def test_executable_requires_exact_root_owned_regular_bytes(self):
        self.assertEqual(os.geteuid(), 0)
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / 'binary'
            binary.write_bytes(b'fixture binary')
            binary.chmod(0o500)
            digest = hashlib.sha256(binary.read_bytes()).hexdigest()
            fd = entry['read_verified_binary'](binary, digest)
            os.close(fd)
            with self.assertRaises(ValueError):
                entry['read_verified_binary'](binary, '0' * 64)
            linked = Path(directory) / 'hardlink'
            os.link(binary, linked)
            with self.assertRaises(ValueError):
                entry['read_verified_binary'](binary, digest)
            linked.unlink()
            symlink = Path(directory) / 'symlink'
            symlink.symlink_to(binary)
            with self.assertRaises(OSError):
                entry['read_verified_binary'](symlink, digest)
            binary.chmod(0o555)
            with self.assertRaises(ValueError):
                entry['read_verified_binary'](binary, digest)


if __name__ == '__main__':
    unittest.main()
