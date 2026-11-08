# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import pathlib
import socket
import subprocess

if (len(pathlib.Path('/proc/net/route').read_text().splitlines()) != 1
        or {name for _, name in socket.if_nameindex()} != {'lo'}):
    raise RuntimeError('Rust archive-cache rehearsal requires network isolation')
env = os.environ.copy()
env['VELNOR_NATIVE_RUST_CACHE_ASSETS'] = '/assets'
subprocess.run(
    ['cargo', 'nextest', 'run', '--locked', '--offline', '-p',
     'velnor-actions-orchestrator', '--lib', '--no-tests', 'fail', '-E',
     'test(generated_candidate_receipt_rejects_unprotected_environment)'],
    cwd='/src', env=env, check=True,
)
