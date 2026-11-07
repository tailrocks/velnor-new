# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
import re
import stat

from release_manager_policy_files import Files, shape
from release_manager_policy_host import Host, check
from release_watch_admission import ManagerError

SUPPORTED_PROFILES = ({'environment_names': ('LANG', 'PATH', 'SGX_AESM_ADDR'),
  'executable_path': '/usr/lib/systemd/systemd',
  'executable_sha256': '508fa02522618fa8dca83df8ab110208853834018bc74d1c727ecb8ef866a714',
  'machine': 'x86_64',
  'mappings': {'/usr/lib/systemd/systemd': '508fa02522618fa8dca83df8ab110208853834018bc74d1c727ecb8ef866a714',
               '/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2': '75c84c6a1522e227864db7d8b40f66b5d6edefcff92538e15a964f1de7855390',
               '/usr/lib/x86_64-linux-gnu/libapparmor.so.1.30.7': '910e5342c97dba26d642ff0121e541672fc4447450a300363884971100947156',
               '/usr/lib/x86_64-linux-gnu/libaudit.so.1.0.0': '47858bf0c1022f1787f87ccc20ab29a6899ee0434ed75921a66b00e327074d45',
               '/usr/lib/x86_64-linux-gnu/libblkid.so.1.1.0': 'f49b80211429fb2b59674975c10c2baf5b1d14767f5475069c720f76f8938ab0',
               '/usr/lib/x86_64-linux-gnu/libbpf.so.1.6.3': 'ada0d47663316aeff20e959d5fcf77d350555fd1cdc0b43c8fd2b81e01cc9c50',
               '/usr/lib/x86_64-linux-gnu/libc.so.6': '305014d0b30755fc12cb419074f6acb4d43c4111e006f2981fd23ee21e25e508',
               '/usr/lib/x86_64-linux-gnu/libcap-ng.so.0.0.0': '1afbc6cddc0e910140b62d74cbacb9f3eb0d9004869314c3b9dc4f9a977b856c',
               '/usr/lib/x86_64-linux-gnu/libcrypt.so.1.1.0': '94fb4e0c1d2db42f957cbc52fbc9643e8009bfa58f12cb149e435010517e0fe6',
               '/usr/lib/x86_64-linux-gnu/libcrypto.so.3': 'd119ef0e18d65f6d20cc4a7621caf4be38bb153ee5f46ba24b0f3eaac6f4a1ff',
               '/usr/lib/x86_64-linux-gnu/libelf-0.194.so': 'd1e1539cc36403b603deb0d4558711e50bc8144eec1d833ece69df81ce01b527',
               '/usr/lib/x86_64-linux-gnu/libkmod.so.2.5.1': 'f638bcff11df48bca725a18f585a5a07786d9c8a671f1b0ea3b31d01946699ab',
               '/usr/lib/x86_64-linux-gnu/libm.so.6': 'eb04310abef45a9fb2f1780a0aced861d0efedb8208a1382687c34d1199d1123',
               '/usr/lib/x86_64-linux-gnu/libmount.so.1.1.0': 'ef8746a202b87a5ed49fedf5cbc16d4de943ae04047699dbcc5f6facdcd88938',
               '/usr/lib/x86_64-linux-gnu/libpcre2-8.so.0.14.0': 'd438b68e0a2034014cb8863c6fd93ceb9e60b98e0181b6ddafbe0b47172498c8',
               '/usr/lib/x86_64-linux-gnu/libseccomp.so.2.6.0': 'ac34d87684bd6cc0023d024db0029fee51c035304795aa82f9ca5bc354dbf735',
               '/usr/lib/x86_64-linux-gnu/libselinux.so.1': '0c6bd58e3a4489ea527aef07287b18316b969c100007f1626960383b5c49239a',
               '/usr/lib/x86_64-linux-gnu/libz.so.1.3.1': 'da78d3cbf7c96c9a8d2f6428c9e3e499488cb921673353a699478d12dea75e44',
               '/usr/lib/x86_64-linux-gnu/libzstd.so.1.5.7': '086554979dc7896c657e676942f71421ccccca788003335512d1a4e62b81671b',
               '/usr/lib/x86_64-linux-gnu/systemd/libsystemd-core-259.so': 'd7e58c1be47141ffc9cde9431ee3c360bbb9ceac2171d20e9cafe9d63cd86a13',
               '/usr/lib/x86_64-linux-gnu/systemd/libsystemd-shared-259.so': '91955fd41c28db2995ebf1508f7922acacf408053bb95b33eda0e89aa6d7aa5a'},
  'policy': {'/etc/systemd/system.conf': {'kind': 'file',
                                          'mode': 420,
                                          'sha256': '24d19b1aa9630cc79f1a683b30b78b2d8660af81829e28ebafbcc8fa92fdd2d3'},
             '/usr/lib/systemd/system.conf.d': {'kind': 'directory', 'mode': 493},
             '/usr/lib/systemd/system.conf.d/10-coredump-debian.conf': {'kind': 'file',
                                                                        'mode': 420,
                                                                        'sha256': '0600cca2c44aaca9b295d260bc15be83dbb3eada10110b6882fee2645294f4f0'},
             '/usr/lib/systemd/system.conf.d/20-coredump-debian.conf': {'kind': 'file',
                                                                        'mode': 420,
                                                                        'sha256': 'c02f5c6e8e151e957fef85e365c37b616531cff3ad2a688e22392838bcb65dab'}},
  'roots': ('/etc/systemd/system.control',
            '/run/systemd/system.control',
            '/run/systemd/transient',
            '/run/systemd/generator.early',
            '/etc/systemd/system',
            '/etc/systemd/system.attached',
            '/run/systemd/system',
            '/run/systemd/system.attached',
            '/run/systemd/generator',
            '/usr/local/lib/systemd/system',
            '/usr/lib/systemd/system',
            '/run/systemd/generator.late'),
  'symlinks': {},
  'version': '259.9-0ubuntu0.1'},
 {'environment_names': ('LANG', 'PATH'),
  'executable_path': '/usr/lib/systemd/systemd',
  'executable_sha256': 'fe6641f80f88837df43069f0fa00efe7dad5a4d8e1ec914b3d6987be68a5fe53',
  'machine': 'aarch64',
  'mappings': {'/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1': '7cdd4ce693d93790bf088dca2e86857f758335b6010d913e086fa384e7704a46',
               '/usr/lib/aarch64-linux-gnu/libaudit.so.1.0.0': 'ff6b5872703d10c4df6a7b5dc6874e192b13e0ce40b2ce42f800b19c000992b2',
               '/usr/lib/aarch64-linux-gnu/libblkid.so.1.1.0': '127e02de220f4a9c9511eb31cde5f847e3170e528ed3f5f313f2512b813d801b',
               '/usr/lib/aarch64-linux-gnu/libc.so.6': '7b17086ce95b15145e6c2c5f8146395f80e2df1af2636fbf78e90c546a70df06',
               '/usr/lib/aarch64-linux-gnu/libcap-ng.so.0.0.0': 'c64f9619d98c36eda6fdd02130438620796e30ca388e7c746e1ed43ec3e83956',
               '/usr/lib/aarch64-linux-gnu/libcrypt.so.1.1.0': '5c5436f75b65261219cbcffe59ed99aed2047d98525526ed62b623884c1135f9',
               '/usr/lib/aarch64-linux-gnu/libcrypto.so.3': 'b8114f6efa2ad981f7b87c3e16c153e425790f1f10b997be2e743401ca7ff858',
               '/usr/lib/aarch64-linux-gnu/libm.so.6': 'ffeae799646fae7a8af80402a88f4bc84bfc110af834f5e756aae2fa0a1a9602',
               '/usr/lib/aarch64-linux-gnu/libmount.so.1.1.0': 'e6e74d3767ab1bd0cab625558b41f28bcb54902d7d6b73ad1dda9abfebfbd41c',
               '/usr/lib/aarch64-linux-gnu/libpcre2-8.so.0.14.0': 'c42c44670e67d050a556b8118a64af68f37988312a2700cd1826f9a9b8cc5ce6',
               '/usr/lib/aarch64-linux-gnu/libseccomp.so.2.6.0': '0303e8861eb173ff1820c0e50d4c77ddde579392dae62ab55f0470dabcb28065',
               '/usr/lib/aarch64-linux-gnu/libselinux.so.1': '3ab6144838b289eb341b4428f8c193199cf313013ccd7c737dca4b4ff83e6e2e',
               '/usr/lib/aarch64-linux-gnu/libz.so.1.3.1': '2d2437c87551b247720b69dd3d761227b4d6f7ee9a20a10ee75e6efc2808b79a',
               '/usr/lib/aarch64-linux-gnu/libzstd.so.1.5.7': '6b9bdeb9c94850c294b909226ab280247faa47e9ff4437f46688a5d328f0cf3b',
               '/usr/lib/aarch64-linux-gnu/systemd/libsystemd-core-259.so': 'aeb311c6fdcbd248534f70660993c05a74dac44cc090f6900d4d65a10c944963',
               '/usr/lib/aarch64-linux-gnu/systemd/libsystemd-shared-259.so': 'cc9d84c0d52e9a2fb62fa9e19c91373a398fc4d55ccb32d562f54c681eb93795',
               '/usr/lib/systemd/systemd': 'fe6641f80f88837df43069f0fa00efe7dad5a4d8e1ec914b3d6987be68a5fe53'},
  'policy': {'/etc/systemd/system.conf': {'kind': 'file',
                                          'mode': 420,
                                          'sha256': 'ceeeab2e0c3050692854c58e2cbffd23638967b48603ec4ef4f27d4fd0b6add4'},
             '/usr/lib/systemd/system.conf.d': {'kind': 'directory', 'mode': 493},
             '/usr/lib/systemd/system.conf.d/10-coredump-debian.conf': {'kind': 'file',
                                                                        'mode': 420,
                                                                        'sha256': '0600cca2c44aaca9b295d260bc15be83dbb3eada10110b6882fee2645294f4f0'}},
  'roots': ('/etc/systemd/system.control',
            '/run/systemd/system.control',
            '/run/systemd/transient',
            '/run/systemd/generator.early',
            '/etc/systemd/system',
            '/etc/systemd/system.attached',
            '/run/systemd/system',
            '/run/systemd/system.attached',
            '/run/systemd/generator',
            '/usr/local/lib/systemd/system',
            '/usr/lib/systemd/system',
            '/run/systemd/generator.late'),
  'symlinks': {},
  'version': '259.9-0ubuntu0.1'})


def unit_scopes(unit):
    if not re.fullmatch(r'velnor-(release|data)-[0-9a-f]{32}\.service', unit):
        raise ManagerError('invalid closed manager policy unit')
    parts = unit.removesuffix('.service').split('-')
    return (unit, *('-'.join(parts[:index]) + '-.service'
                   for index in range(len(parts) - 1, 0, -1)), 'service')


def require_profile(host):
    if not isinstance(host, Host):
        raise ManagerError('authenticated manager policy identity missing')
    process, version, roots, environment = host.facts
    profiles = [profile for profile in SUPPORTED_PROFILES
                if profile['machine'] == os.uname().machine and profile['version'] == version
                and profile['executable_sha256'] == process[4][0] and profile['roots'] == roots
                and profile['executable_path'] == process[6]
                and profile['mappings'] == {path: value[0] for path, value in process[5]}
                and profile['environment_names'] == environment[0]]
    if process[0] != 1 or len(profiles) != 1:
        raise ManagerError('unsupported authenticated manager policy')
    return profiles[0]


class GeneratedDeadline:
    def __init__(self, unit, directory, descriptor, payload):
        unit_scopes(unit)
        if (not re.fullmatch(rb'\[Unit\]\nJobTimeoutSec=[1-9][0-9]{0,4}s\n', payload)):
            raise ManagerError('invalid generated deadline policy')
        info = os.fstat(directory)
        file = os.fstat(descriptor)
        if (info.st_uid != 0 or not stat.S_ISDIR(info.st_mode) or stat.S_IMODE(info.st_mode) != 0o700
                or file.st_uid != 0 or not stat.S_ISREG(file.st_mode)
                or stat.S_IMODE(file.st_mode) != 0o600 or file.st_nlink != 1
                or os.pread(descriptor, len(payload) + 1, 0) != payload):
            raise ManagerError('generated deadline custody invalid')
        self.directory = os.dup(directory)
        try:
            self.descriptor = os.dup(descriptor)
        except BaseException:
            os.close(self.directory)
            raise
        self.directory_identity = shape(info)
        self.file_identity = shape(file)
        self.path = '/run/systemd/system/' + unit + '.d'
        self.sha256 = hashlib.sha256(payload).hexdigest()

    def close(self):
        if self.directory is not None:
            os.close(self.directory)
            os.close(self.descriptor)
            self.directory = None
            self.descriptor = None


class Admission:
    def __init__(self, host, manager, unit, generated, deadline):
        check(deadline)
        scopes = unit_scopes(unit)
        if generated.path != '/run/systemd/system/' + unit + '.d':
            raise ManagerError('manager policy unit binding invalid')
        host.revalidate(manager, deadline)
        profile = require_profile(host)
        roots = host.facts[2]
        self.host = host
        self.manager = manager
        self.deadline = deadline
        self.environment_names = host.facts[3][0]
        self.files = Files(profile, deadline)
        try:
            for path, expected in profile['mappings'].items():
                self.files.code(path, expected)
            for root in roots:
                self.files.directory(root)
                for scope in scopes:
                    for suffix in ('', '.d', '.wants', '.requires', '.upholds'):
                        self.files.scope(root, scope + suffix, generated)
            for root in ('/etc/systemd', '/run/systemd', '/usr/local/lib/systemd', '/usr/lib/systemd'):
                self.files.directory(root)
                for scope in ('system.conf', 'system.conf.d'):
                    self.files.scope(root, scope, generated)
            self.revalidate()
        except BaseException:
            self.close()
            raise

    def revalidate(self):
        self.host.revalidate(self.manager, self.deadline)
        self.files.revalidate()
        self.host.revalidate(self.manager, self.deadline)

    def close(self):
        self.files.close()
