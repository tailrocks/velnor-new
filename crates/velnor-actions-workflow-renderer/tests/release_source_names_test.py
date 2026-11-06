# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import fcntl
import importlib.util
import json
import os
from pathlib import Path

spec = importlib.util.spec_from_file_location('executor', Path(__file__).resolve().parents[1] / 'src/release_source_executor.py')
executor = importlib.util.module_from_spec(spec)
spec.loader.exec_module(executor)

files = [{'path': name, 'sha256': 'a' * 64, 'kind': {'type': 'regular', 'executable': False}} for name in ('Cargo.lock', 'Cargo.toml')]
identity = {'repository': 'fixture/example', 'repository_id': 2, 'owner_id': 3, 'run_id': 4, 'run_attempt': 1, 'workflow_sha': 'f' * 40, 'intent_sha256': 'e' * 64}
tools = dict.fromkeys(('cargo', 'rustc', 'rustdoc', 'semver', 'git', 'git_http', 'cc', 'release_plz'), 'a' * 64)

def check(reader, value, valid):
    descriptor = os.memfd_create('source-name-test', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
    try:
        os.write(descriptor, json.dumps(value).encode())
        fcntl.fcntl(descriptor, fcntl.F_ADD_SEALS, fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
        try:
            reader(descriptor)
        except executor.SourceExecutorError:
            if valid:
                raise
        else:
            if not valid:
                raise AssertionError('invalid package name accepted')
    finally:
        os.close(descriptor)

for name, valid in [('_codec', True), ('alpha-2', True), ('2codec', False), ('-codec', False), ('', False), ('é', False)]:
    source = {'identity': identity, 'source_sha': 'a' * 40, 'rust_version': '1.99.1', 'manifest': 'Cargo.toml', 'authentication': 'trusted-publishing', 'source_files': files, 'packages': [{'name': name, 'version': '1.2.3', 'manifest': 'Cargo.toml', 'features': [], 'upload_config_sha256': 'b' * 64}], 'tools': tools}
    semver = {'source_sha': 'a' * 40, 'package': name, 'target_version': '1.2.3', 'manifest': 'Cargo.toml', 'features': [], 'baseline_version': '1.2.2', 'baseline_checksum': 'b' * 64, 'files': files}
    check(executor.read_context, source, valid)
    check(executor.read_semver_context, semver, valid)
