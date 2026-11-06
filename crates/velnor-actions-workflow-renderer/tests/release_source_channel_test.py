# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import array
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import socket
import struct
import sys
import threading
import tempfile
import shutil
import time
import unittest

root = Path(__file__).resolve().parents[1] / 'src'

def load(name):
    spec = importlib.util.spec_from_file_location(name, root / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module

channel = load('release_source_channel')
executor = load('release_source_executor')


def deadline():
    return time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 2_000_000_000


def send_frame(control, data):
    control.sendall(struct.pack('>I', len(data)) + data)


def context_descriptor():
    value = {'identity': {'repository': 'fixture/example', 'repository_id': 2, 'owner_id': 3, 'run_id': 4, 'run_attempt': 1, 'workflow_sha': 'f' * 40, 'intent_sha256': 'e' * 64}, 'source_sha': 'a' * 40, 'rust_version': '1.99.1', 'manifest': 'Cargo.toml',
             'source_files': [{'path': 'Cargo.toml', 'sha256': 'b' * 64,
                               'kind': {'type': 'regular', 'executable': False}}],
             'packages': [{'name': 'fixture', 'version': None, 'manifest': None, 'features': []}],
             'tools': {'cargo': 'c' * 64, 'rustc': 'd' * 64, 'rustdoc': 'e' * 64}}
    fd = os.memfd_create('fixture-context', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
    os.write(fd, json.dumps(value).encode())
    fcntl.fcntl(fd, fcntl.F_ADD_SEALS, fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
    return fd


class SourceChannelTest(unittest.TestCase):
    def test_rejects_replayed_extra_and_unknown_requests(self):
        invalid = [b'{"seq":2,"op":"metadata"}', b'{"seq":1,"op":"metadata","program":"/bin/sh"}',
                   b'{"seq":1,"op":"upload"}', b'{"seq":1,"seq":1,"op":"metadata"}',
                   b'{"seq":1,"op":"package","index":true}']
        for value in invalid:
            left, right = socket.socketpair()
            try:
                send_frame(left, value)
                with self.subTest(value=value), self.assertRaises(channel.SourceChannelError):
                    channel.request(right, 1, deadline())
            finally:
                left.close()
                right.close()

    def test_rejects_descriptors_attached_to_ordinary_request(self):
        left, right = socket.socketpair()
        descriptor = os.open('/dev/null', os.O_RDONLY)
        try:
            value = b'{"seq":1,"op":"metadata"}'
            before = len(os.listdir('/proc/self/fd'))
            left.sendmsg([struct.pack('>I', len(value)) + value],
                         [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array('i', [descriptor]))])
            with self.assertRaises(channel.SourceChannelError):
                channel.request(right, 1, deadline())
            self.assertEqual(len(os.listdir('/proc/self/fd')), before)
        finally:
            os.close(descriptor)
            left.close()
            right.close()

    def test_partial_frame_obeys_deadline(self):
        left, right = socket.socketpair()
        try:
            left.sendall(b'\0\0')
            end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 200_000_000
            with self.assertRaises(channel.SourceChannelError):
                channel.request(right, 1, end)
        finally:
            left.close()
            right.close()

    def test_extra_or_unsealed_descriptors_are_closed_and_rejected(self):
        for count, sealed in ((1, False), (2, True), (8, True)):
            left, right = socket.socketpair()
            fd = context_descriptor() if sealed else os.memfd_create('unsealed', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
            try:
                before = len(os.listdir('/proc/self/fd'))
                left.sendmsg([b'C'], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array('i', [fd] * count))])
                with self.assertRaises((channel.SourceChannelError, executor.SourceExecutorError)):
                    channel.receive_context(right, executor.read_context, deadline())
                self.assertEqual(len(os.listdir('/proc/self/fd')), before)
            finally:
                os.close(fd)
                left.close()
                right.close()

    def test_package_before_metadata_and_binding_closes_channel(self):
        directory = tempfile.mkdtemp(dir='/run', prefix='velnor-publisher-')
        self.addCleanup(shutil.rmtree, directory)
        os.chmod(directory, 0o711)
        info = os.stat(directory)
        left, right = socket.socketpair()
        errors = []
        def run():
            try:
                channel.serve(right, None, None, executor, directory, deadline(), (info.st_dev, info.st_ino))
            except BaseException as error:
                errors.append(error)
            finally:
                right.close()
        worker = threading.Thread(target=run)
        worker.start()
        fd = context_descriptor()
        try:
            send_frame(left, b'{"seq":1,"op":"init"}')
            left.sendmsg([b'C'], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array('i', [fd]))])
            self.assertEqual(left.recv(12), struct.pack('>QI', 1, 0))
            send_frame(left, b'{"seq":2,"op":"package-verify","index":0}')
            left.settimeout(3)
            self.assertEqual(left.recv(1), b'')
            worker.join(timeout=3)
            self.assertFalse(worker.is_alive())
            self.assertEqual(len(errors), 1)
            self.assertIsInstance(errors[0], executor.SourceExecutorError)
        finally:
            os.close(fd)
            left.close()
            worker.join(timeout=3)


if __name__ == '__main__':
    unittest.main()
