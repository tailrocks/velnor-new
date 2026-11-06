# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import base64
import os
from pathlib import Path
import tempfile
import time
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
    result['VELNOR_RELEASE_BUDGET'] = (
        f"v1:{result['GITHUB_RUN_ID']}:{result['GITHUB_RUN_ATTEMPT']}:{result['GITHUB_JOB']}:"
        f"{time.clock_gettime_ns(getattr(time, "CLOCK_BOOTTIME", time.CLOCK_MONOTONIC)) + 60_000_000_000}"
    )
    if mode == 'trusted-publishing':
        result.update({'ACTIONS_ID_TOKEN_REQUEST_TOKEN': 'fixture-oidc',
                       'ACTIONS_ID_TOKEN_REQUEST_URL': 'https://example.actions.githubusercontent.com/token'})
    else:
        result['CARGO_REGISTRY_TOKEN'] = 'fixture-registry'
    return result


class PublisherEntryTest(unittest.TestCase):
    @unittest.skipUnless(hasattr(time, "CLOCK_BOOTTIME"), "Linux BOOTTIME required")
    def test_deadline_uses_the_original_job_budget(self):
        values = context('trusted-publishing')
        now = time.clock_gettime_ns(time.CLOCK_BOOTTIME)
        deadline = now + 60_000_000_000
        budget = f"v1:{values['GITHUB_RUN_ID']}:{values['GITHUB_RUN_ATTEMPT']}:{values['GITHUB_JOB']}:{deadline}"
        self.assertEqual(entry['bound_deadline'](values, budget), deadline)
        for bad in (
            budget.replace(':9:', ':10:'),
            budget.replace(':release-publish:', ':release-publish-bootstrap:'),
            f"v1:9:1:release-publish:{now - 1}",
            f"v1:9:1:release-publish:{now + 1801_000_000_000}",
            budget + ':extra',
        ):
            with self.subTest(budget=bad), self.assertRaises(ValueError):
                entry['bound_deadline'](values, bad)

    def test_embedded_supervisor_modules_load_from_fixed_bytes(self):
        root = Path(__file__).resolve().parents[1] / 'src'
        for name, member in (
            ('release_retained_parent', 'run'),
            ('release_source_executor', 'SourceSession'),
            ('release_source_channel', 'serve'),
            ('release_upload_operation', 'upload'),
            ('release_forge_broker', 'ForgeBroker'),
        ):
            encoded = base64.b64encode((root / (name + '.py')).read_bytes()).decode('ascii')
            module = entry['module']('fixture_' + name, encoded)
            self.assertTrue(hasattr(module, member), name)

    def test_fixed_helper_staging_rejects_replacement_and_symlinks(self):
        self.assertEqual(os.geteuid(), 0)
        with tempfile.TemporaryDirectory(dir='/run', prefix='velnor-publisher-') as directory:
            old_mask = os.umask(0o077)
            try:
                path = entry['stage_fixed'](directory, 'release_source_exec.py', b'fixed\n', 0o555)
            finally:
                os.umask(old_mask)
            self.assertEqual(Path(path).read_bytes(), b'fixed\n')
            self.assertEqual(Path(path).stat().st_mode & 0o777, 0o555)
            with self.assertRaises(OSError):
                entry['stage_fixed'](directory, 'release_source_exec.py', b'other', 0o555)
            (Path(directory) / 'release_other.py').symlink_to(path)
            with self.assertRaises(OSError):
                entry['stage_fixed'](directory, 'release_other.py', b'other', 0o555)
            with self.assertRaises(ValueError):
                entry['stage_fixed'](directory, '../escape.py', b'other', 0o555)

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
            ('RUNNER_ENVIRONMENT', 'self-hosted'),
        ):
            values = context('trusted-publishing')
            values[key] = value
            with self.subTest(key=key), patch.dict(os.environ, values, clear=True):
                with self.assertRaises(ValueError):
                    entry['read_context']('trusted-publishing')

    def test_context_defers_publishing_credentials_until_upload(self):
        for mode in ('trusted-publishing', 'bootstrap'):
            values = context(mode)
            for key in ('ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_URL', 'CARGO_REGISTRY_TOKEN'):
                values.pop(key, None)
            with self.subTest(mode=mode), patch.dict(os.environ, values, clear=True):
                accepted = entry['read_context'](mode)
                self.assertNotIn('CARGO_REGISTRY_TOKEN', accepted)
                self.assertNotIn('ACTIONS_ID_TOKEN_REQUEST_TOKEN', accepted)
                self.assertNotIn('ACTIONS_ID_TOKEN_REQUEST_URL', accepted)

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
