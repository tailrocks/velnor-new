# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import io
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile

script, version = sys.argv[1:]


def invoke(command, cwd, environment, *, success=True):
    result = subprocess.run(command, cwd=cwd, env=environment, capture_output=True, text=True)
    if success and result.returncode:
        raise AssertionError(f'{command}: {result.stderr}')
    return result


def run_case(missing):
    with tempfile.TemporaryDirectory(prefix='preview-prefetch-') as directory:
        root = Path(directory)
        index, downloads, workspace = root / 'index', root / 'downloads', root / 'workspace'
        index.mkdir()
        downloads.mkdir()
        home = root / 'cargo-home'
        home.mkdir()
        outside = root / 'outside'
        outside.mkdir()
        environment = os.environ.copy()
        for key in ('GITHUB_TOKEN', 'GH_TOKEN', 'MISE_GITHUB_TOKEN', 'ACTIONS_RUNTIME_TOKEN',
                    'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_URL'):
            environment.pop(key, None)
        environment.update(CARGO_HOME=str(home), GITHUB_WORKSPACE=str(workspace),
                           RUNNER_TEMP=str(outside), MISE_EXEC_AUTO_INSTALL='0', CARGO_NET_RETRY='0',
                           GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull)
        rust_cargo = invoke(['rustup', 'which', '--toolchain', version, 'cargo'], outside, environment).stdout.strip()
        environment['PATH'] = str(Path(rust_cargo).parent) + os.pathsep + environment['PATH']
        mise = Path(shutil.which('mise')).resolve()
        rust_bin = Path(rust_cargo).parent
        private_bin = home / 'bin'
        private_bin.mkdir()
        for tool in ('cargo', 'rustc', 'rustdoc'):
            (private_bin / tool).symlink_to(rust_bin / tool)
        mise_data = root / 'mise-data'
        installations = mise_data / 'installs/rust'
        installations.mkdir(parents=True)
        (installations / version).symlink_to(private_bin, target_is_directory=True)
        environment.update(MISE_DATA_DIR=str(mise_data), MISE_CACHE_DIR=str(root / 'mise-cache'),
                           MISE_STATE_DIR=str(root / 'mise-state'), MISE_CONFIG_DIR=str(root / 'mise-config'),
                           RUSTUP_HOME=environment.get('RUSTUP_HOME', str(Path.home() / '.rustup')),
                           RUSTUP_TOOLCHAIN=version)
        environment['PATH'] = str(mise.parent) + os.pathsep + environment['PATH']
        cargo = [str(mise), '--no-config', '--no-env', '--no-hooks', 'exec',
                 'rust@' + version, '--', 'cargo']
        (index / 'config.json').write_text(json.dumps({
            'dl': downloads.as_uri() + '/{crate}/{version}/download'}))
        for name in ('first', 'second'):
            archive = io.BytesIO()
            with tarfile.open(fileobj=archive, mode='w:gz') as tar:
                for name_part, data in {
                    'Cargo.toml': f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2021"\n'.encode(),
                    'src/lib.rs': b'pub fn value() {}\n',
                }.items():
                    info = tarfile.TarInfo(f'{name}-0.1.0/{name_part}')
                    info.size = len(data)
                    tar.addfile(info, io.BytesIO(data))
            data = archive.getvalue()
            archive_path = downloads / name / '0.1.0' / 'download'
            archive_path.parent.mkdir(parents=True)
            archive_path.write_bytes(data)
            entry = index / name[:2] / name[2:4] / name
            entry.parent.mkdir(parents=True, exist_ok=True)
            entry.write_text(json.dumps({'name': name, 'vers': '0.1.0', 'deps': [],
                'cksum': hashlib.sha256(data).hexdigest(), 'features': {}, 'yanked': False}) + '\n')
        invoke(['git', 'init', '-q', str(index)], root, environment)
        invoke(['git', '-C', str(index), 'add', '.'], root, environment)
        invoke(['git', '-C', str(index), '-c', 'user.name=Fixture', '-c',
                'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture'], root, environment)
        (home / 'config.toml').write_text('[source.crates-io]\nreplace-with="local"\n'
            '[source.local]\nregistry="' + index.as_uri() + '"\n')
        manifests = []
        for relative, dependency in (('', 'first'), ('crates/velnor-runner', 'second')):
            project = workspace / relative
            (project / 'src').mkdir(parents=True)
            (project / 'src/lib.rs').write_text('')
            manifest = project / 'Cargo.toml'
            manifest.write_text(f'[package]\nname="fixture-{dependency}"\nversion="0.1.0"\nedition="2021"\n'
                                f'[dependencies]\n{dependency}="=0.1.0"\n')
            manifests.append(manifest)
            invoke(cargo + ['generate-lockfile', '--manifest-path', str(manifest)], outside, environment)
        shutil.rmtree(home / 'registry')
        if missing:
            (downloads / 'second/0.1.0/download').unlink()
        marker = root / 'qualified'
        phase = script + '\nprintf done > "$PREFETCH_MARKER"\n'
        environment['PREFETCH_MARKER'] = str(marker)
        result = invoke(['bash', '-e', '-c', phase], outside, environment, success=False)
        if missing:
            assert result.returncode != 0 and not marker.exists(), result.stderr
        else:
            assert result.returncode == 0 and marker.exists(), result.stderr
            for manifest in manifests:
                result = invoke(cargo + ['metadata', '--format-version', '1', '--locked', '--offline',
                                         '--manifest-path', str(manifest)], outside, environment, success=False)
                assert result.returncode == 0, f'offline metadata failed for {manifest}: {result.stderr}'


run_case(False)
run_case(True)
