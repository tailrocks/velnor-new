#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import pathlib
import subprocess
import tempfile

SCRIPT = pathlib.Path(__file__).with_name('observe-hosted-checkout.py')


def main():
    with tempfile.TemporaryDirectory() as directory:
        root = pathlib.Path(directory).resolve()
        (root / '.git/objects/pack').mkdir(parents=True)
        (root / '.git/objects/info').mkdir()
        config = root / '.git/config'
        config.write_text('[core]\n bare = false\n# private-fixture-sentinel\n')
        env = {
            'GITHUB_EVENT_NAME': 'workflow_dispatch', 'ImageOS': 'ubuntu26', 'RUNNER_ARCH': 'X64', 'RUNNER_OS': 'Linux',
            'GITHUB_SHA': 'a' * 40, 'GITHUB_WORKFLOW_SHA': 'a' * 40,
            'GITHUB_REPOSITORY': 'tailrocks/velnor-new',
            'GITHUB_WORKFLOW_REF': 'tailrocks/velnor-new/.github/workflows/source-qualification.yml@refs/heads/fixture',
            'GITHUB_RUN_ID': '1', 'GITHUB_RUN_ATTEMPT': '1', 'ImageVersion': '20261108.1',
            'GITHUB_WORKSPACE': str(root), 'RUNNER_TEMP': str(root),
        }
        def run():
            return subprocess.run(['/usr/bin/python3', '-I', str(SCRIPT)], env=env, capture_output=True, timeout=10)
        before = sorted(str(path.relative_to(root)) for path in root.rglob('*'))
        original_config = config.read_bytes()
        result = run()
        if result.returncode != 0:
            raise ValueError('valid checkout observation rejected')
        observed = json.loads(result.stdout)
        if b'private-fixture-sentinel' in result.stdout:
            raise ValueError('config content leaked')
        if not observed['observation_only'] or observed['config_worktree_present'] or not observed['objects_tree_compatible']:
            raise ValueError('incorrect initial observation')
        extra = root / '.git/config.worktree'
        extra.touch()
        result = run()
        if result.returncode != 0 or not json.loads(result.stdout)['config_worktree_present']:
            raise ValueError('worktree config not observed')
        extra.unlink()
        if config.read_bytes() != original_config or sorted(str(path.relative_to(root)) for path in root.rglob('*')) != before:
            raise ValueError('observation changed fixture')
        object_link = root / '.git/objects/linked-object'
        os.symlink(root, object_link)
        result = run()
        if result.returncode != 0 or json.loads(result.stdout)['objects_tree_compatible']:
            raise ValueError('object link not detected')
        object_link.unlink()
        object_file = root / '.git/objects/object-file'
        object_file.write_bytes(b'fixture')
        hardlink = root / '.git/objects/object-hardlink'
        os.link(object_file, hardlink)
        result = run()
        if result.returncode != 0 or json.loads(result.stdout)['objects_tree_compatible']:
            raise ValueError('object hardlink not detected')
        hardlink.unlink()
        object_file.chmod(0o666)
        result = run()
        if result.returncode != 0 or json.loads(result.stdout)['objects_tree_compatible']:
            raise ValueError('writable object not detected')
        object_file.unlink()
        root.chmod(0o777)
        result = run()
        if result.returncode != 0 or not json.loads(result.stdout)['runner_temp_group_or_other_writable']:
            raise ValueError('writable runner temp not detected')
        root.chmod(0o700)
        env['GITHUB_WORKFLOW_SHA'] = 'b' * 40
        if run().returncode == 0:
            raise ValueError('mismatched workflow identity accepted')
        env['GITHUB_WORKFLOW_SHA'] = 'a' * 40
        objects = root / '.git/objects'
        original = root / '.git/objects-original'
        objects.rename(original)
        os.symlink(original, objects)
        if run().returncode == 0:
            raise ValueError('object directory symlink accepted')


if __name__ == '__main__':
    main()
