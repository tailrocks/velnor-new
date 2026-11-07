# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
from pathlib import Path
import subprocess
import sys
import tempfile

fixture, script, version = sys.argv[1:]
rustup = os.environ.get('RUSTUP_HOME', str(Path.home() / '.rustup'))
probe_environment = {'HOME': str(Path.home()), 'PATH': os.environ['PATH'], 'RUSTUP_HOME': rustup}
cargo = Path(subprocess.run(['rustup', 'which', '--toolchain', version, 'cargo'],
                            env=probe_environment, capture_output=True, text=True, check=True).stdout.strip())
with tempfile.TemporaryDirectory(prefix='preview-prefetch-parent-') as directory:
    root = Path(directory)
    home = root / 'home'
    home.mkdir()
    environment = {'HOME': str(home), 'PATH': os.environ['PATH'], 'RUSTUP_HOME': rustup}
    index = root / 'index'
    index.mkdir()
    (index / 'config.json').write_text('{"dl":"file:///missing/{crate}/{version}/download"}')
    environment.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull, GIT_TEMPLATE_DIR='')
    for command in (['git', 'init', '-q', str(index)], ['git', '-C', str(index), 'add', '.'],
                    ['git', '-C', str(index), '-c', 'user.name=Fixture', '-c',
                     'user.email=fixture@example.invalid', 'commit', '-qm', 'empty']):
        subprocess.run(command, env=environment, check=True, capture_output=True)
    cargo_home = root / 'cargo'
    cargo_home.mkdir()
    (cargo_home / 'config.toml').write_text('[source.crates-io]\nreplace-with="empty"\n'
        '[source.empty]\nregistry="' + index.as_uri() + '"\n')
    rustup_home = root / 'rustup'
    (rustup_home / 'toolchains').mkdir(parents=True)
    (rustup_home / 'toolchains' / cargo.parent.parent.name).symlink_to(cargo.parent.parent,
                                                                      target_is_directory=True)
    environment.update(MISE_CARGO_HOME=str(cargo_home), MISE_RUSTUP_HOME=str(rustup_home))
    result = subprocess.run([sys.executable, fixture, script, version], env=environment, check=False)
    raise SystemExit(result.returncode)
