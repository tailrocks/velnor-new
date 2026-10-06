"""Bootstrap and process boundary regressions; no issuer credentials used."""
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import cache_receipt_gh as gh
from cache_receipt_common import ColdReceipt


class OutputProcess:
    def __init__(self, output=b'{}', errors=b'', running=False):
        self.stdout = self.pipe(output)
        self.stderr = self.pipe(errors)
        self.running = running
        self.killed = False

    @staticmethod
    def pipe(data):
        reader, writer = os.pipe()
        os.write(writer, data)
        os.close(writer)
        return os.fdopen(reader, 'rb', buffering=0)

    def poll(self):
        return None if self.running else 0

    def kill(self):
        self.killed = True
        self.running = False

    def wait(self, timeout=None):
        self.running = False
        return 0


class GhBoundaryTests(unittest.TestCase):
    def test_unqualified_projection_never_executes(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = str(Path(directory).resolve())
            marker = Path(directory) / 'executed'
            payload = Path(directory) / 'fake-gh'
            payload.write_text('#!/bin/sh\ntouch ' + str(marker) + '\n')
            payload.chmod(0o700)
            with self.assertRaisesRegex(ColdReceipt, 'distribution_unqualified'):
                gh.qualified_gh(str(payload), b'{}', directory, 'owner/repo')
            self.assertFalse(marker.exists())

    def test_environment_excludes_credentials_and_startup(self):
        hostile = {'GH_TOKEN': 'secret', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN': 'secret',
                   'ACTIONS_RUNTIME_TOKEN': 'secret', 'SSLKEYLOGFILE': '/tmp/leak',
                   'HTTPS_PROXY': 'http://attacker', 'BASH_ENV': '/tmp/evil',
                   'PYTHONPATH': '/tmp/evil', 'LD_PRELOAD': '/tmp/evil'}
        with mock.patch.dict(os.environ, hostile):
            actual = gh._environment('/owned/home')
        self.assertTrue(set(actual).isdisjoint(hostile))
        self.assertEqual(actual['GH_CONFIG_DIR'], '/owned/home/gh')

    def test_fifo_and_links_rejected_before_read(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = str(Path(directory).resolve())
            fifo = Path(directory) / 'fifo'
            os.mkfifo(fifo)
            with self.assertRaisesRegex(ColdReceipt, 'binary_shape'):
                gh._regular_source(str(fifo))
            real = Path(directory) / 'real'
            real.mkdir()
            binary = real / 'gh'
            binary.write_bytes(b'not-an-executable')
            link = Path(directory) / 'link'
            link.symlink_to(real)
            with self.assertRaisesRegex(ColdReceipt, 'binary_path'):
                gh._regular_source(str(link / 'gh'))
            hard = real / 'hard'
            os.link(binary, hard)
            with self.assertRaisesRegex(ColdReceipt, 'binary_shape'):
                gh._regular_source(str(binary))

    def test_strict_process_json_and_environment(self):
        for output in (b'{"a":1,"a":2}', b'{"a":NaN}', b'{"a":1e999}'):
            process = OutputProcess(output)
            with mock.patch.object(gh.subprocess, 'Popen', return_value=process) as spawn:
                with self.assertRaises(ColdReceipt):
                    gh._run(42, ['attestation', 'verify'], '/owned/home')
            self.assertEqual(spawn.call_args.kwargs['env'], gh._environment('/owned/home'))
            self.assertEqual(spawn.call_args.kwargs['pass_fds'], (42,))

    def test_bounded_output_kills_process(self):
        process = OutputProcess(b'{}', running=True)
        with mock.patch.object(gh.subprocess, 'Popen', return_value=process):
            with mock.patch.object(gh, '_OUTPUT_LIMIT', 1):
                with self.assertRaisesRegex(ColdReceipt, 'output_size'):
                    gh._run(42, [], '/owned/home')
        self.assertTrue(process.killed)
        self.assertTrue(process.stdout.closed)
        self.assertTrue(process.stderr.closed)

    @unittest.skipUnless(hasattr(os, 'memfd_create'), 'Linux sealed execution')
    def test_sealed_binary_is_exact_and_immutable(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = str(Path(directory).resolve())
            path = Path(directory) / 'binary'
            contents = b'synthetic-nonexecuted-binary'
            path.write_bytes(contents)
            record = {'tool': 'gh', 'version': '2.102.0', 'machine': os.uname().machine,
                      'binary_sha256': hashlib.sha256(contents).hexdigest(),
                      'qualification_sha256': 'a' * 64}
            descriptor = gh._seal_binary(str(path), record)
            try:
                self.assertEqual(os.pread(descriptor, len(contents), 0), contents)
                with self.assertRaises(OSError):
                    os.pwrite(descriptor, b'changed', 0)
            finally:
                os.close(descriptor)
            record['binary_sha256'] = 'b' * 64
            with self.assertRaisesRegex(ColdReceipt, 'binary_digest'):
                gh._seal_binary(str(path), record)


if __name__ == '__main__':
    unittest.main()
