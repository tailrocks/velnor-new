#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import pathlib
import re
import stat
import subprocess


def probe(argv):
    if not os.path.isfile(argv[0]):
        return 'unavailable'
    try:
        completed = subprocess.run(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, timeout=3, check=False,
                                   env={'PATH': '/usr/bin:/bin'})
    except (OSError, subprocess.TimeoutExpired):
        return 'error_or_timeout'
    return 'available' if completed.returncode == 0 else 'rejected'


def sandbox_observation():
    return {
        'unprivileged_user_network_namespace': probe(['/usr/bin/unshare', '--user', '--map-root-user', '--net', '/usr/bin/true']),
        'bubblewrap_network_namespace': probe(['/usr/bin/bwrap', '--unshare-net', '--ro-bind', '/', '/', '--', '/usr/bin/true']),
        'systemd_user_scope': probe(['/usr/bin/systemd-run', '--user', '--scope', '--quiet', '/usr/bin/true']),
        'cgroup_v2_present': os.path.isfile('/sys/fs/cgroup/cgroup.controllers'),
        'cgroup_root_writable': os.access('/sys/fs/cgroup', os.W_OK),
        'termination_cleanup_tested': False,
        'timeout_scope': 'direct-child-only',
        'systemd_environment': 'path-only-no-user-bus',
    }


def main():
    if os.environ.get('GITHUB_EVENT_NAME') != 'workflow_dispatch' or os.environ.get('ImageOS') != 'ubuntu26' or os.environ.get('RUNNER_ARCH') != 'X64' or os.environ.get('RUNNER_OS') != 'Linux':
        raise ValueError('manual Ubuntu 26.04 x86-64 run required')
    sha = os.environ.get('GITHUB_SHA', '')
    if not re.fullmatch(r'[0-9a-f]{40}', sha) or sha != os.environ.get('GITHUB_WORKFLOW_SHA'):
        raise ValueError('source identity mismatch')
    repository = os.environ.get('GITHUB_REPOSITORY', '')
    workflow_ref = os.environ.get('GITHUB_WORKFLOW_REF', '')
    run_id = os.environ.get('GITHUB_RUN_ID', '')
    attempt = os.environ.get('GITHUB_RUN_ATTEMPT', '')
    image_version = os.environ.get('ImageVersion', '')
    if (not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository)
            or not workflow_ref.startswith(repository + '/.github/workflows/source-qualification.yml@refs/heads/')
            or len(workflow_ref) > 512 or not re.fullmatch(r'[1-9][0-9]*', run_id)
            or not re.fullmatch(r'[1-9][0-9]*', attempt) or not re.fullmatch(r'[0-9.]+', image_version)):
        raise ValueError('workflow identity invalid')
    runner = pathlib.Path(os.environ['RUNNER_TEMP'])
    if not runner.is_absolute() or runner.resolve(strict=True) != runner:
        raise ValueError('runner path invalid')
    workspace = pathlib.Path(os.environ['GITHUB_WORKSPACE'])
    if not workspace.is_absolute() or workspace.resolve(strict=True) != workspace:
        raise ValueError('workspace path invalid')
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    descriptors = []
    try:
        def open_dir(name, parent=None):
            fd = os.open(name, flags, dir_fd=parent)
            descriptors.append(fd)
            return fd
        runner_fd = open_dir(runner)
        runner_metadata = os.fstat(runner_fd)
        workspace_fd = open_dir(workspace)
        git_fd = open_dir('.git', workspace_fd)
        objects_fd = open_dir('objects', git_fd)
        info_fd = open_dir('info', objects_fd)
        pack_fd = open_dir('pack', objects_fd)
        git = os.fstat(git_fd)
        objects = os.fstat(objects_fd)
        config = os.stat('config', dir_fd=git_fd, follow_symlinks=False)
        object_entries = 0
        object_tree_compatible = True
        def inspect_tree(fd, depth):
            nonlocal object_entries, object_tree_compatible
            if depth > 4:
                raise ValueError('object depth limit')
            with os.scandir(fd) as entries:
                for entry in entries:
                    object_entries += 1
                    if object_entries > 100000:
                        raise ValueError('object entry limit')
                    metadata = os.stat(entry.name, dir_fd=fd, follow_symlinks=False)
                    if metadata.st_uid != os.geteuid() or metadata.st_mode & 0o022:
                        object_tree_compatible = False
                    if stat.S_ISDIR(metadata.st_mode):
                        child = os.open(entry.name, flags, dir_fd=fd)
                        try:
                            inspect_tree(child, depth + 1)
                        finally:
                            os.close(child)
                    elif not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1 or metadata.st_size > 2 * 1024 * 1024 * 1024:
                        object_tree_compatible = False
        inspect_tree(objects_fd, 0)
        def present(fd, name):
            try:
                os.stat(name, dir_fd=fd, follow_symlinks=False)
                return True
            except FileNotFoundError:
                return False
        count = 0
        promisor = False
        with os.scandir(pack_fd) as entries:
            for entry in entries:
                count += 1
                if count > 100000:
                    raise ValueError('pack entry limit')
                promisor |= entry.name.endswith('.promisor')
        result = {
            'observation_only': True,
            'sandbox_capabilities': sandbox_observation(),
            'config_keys_not_examined': True,
            'runner_temp_owned_by_runner': runner_metadata.st_uid == os.geteuid(),
            'runner_temp_group_or_other_writable': bool(runner_metadata.st_mode & 0o022),
            'objects_tree_compatible': object_tree_compatible,
            'source_sha': sha,
            'workflow_sha': sha,
            'run_id': run_id,
            'run_attempt': attempt,
            'runner_image': image_version,
            'config_worktree_present': present(git_fd, 'config.worktree'),
            'commondir_present': present(git_fd, 'commondir'),
            'alternates_present': present(info_fd, 'alternates'),
            'http_alternates_present': present(info_fd, 'http-alternates'),
            'promisor_marker_present': promisor,
            'git_dir_owned_by_runner': git.st_uid == os.geteuid(),
            'git_dir_group_or_other_writable': bool(git.st_mode & 0o022),
            'config_regular_single_link': stat.S_ISREG(config.st_mode) and config.st_nlink == 1,
            'config_owned_by_runner': config.st_uid == os.geteuid(),
            'config_group_or_other_writable': bool(config.st_mode & 0o022),
            'config_within_limit': config.st_size <= 128 * 1024,
            'objects_dir_owned_by_runner': objects.st_uid == os.geteuid(),
            'objects_dir_group_or_other_writable': bool(objects.st_mode & 0o022),
        }
        print(json.dumps(result, sort_keys=True, separators=(',', ':')))
    finally:
        for fd in reversed(descriptors):
            os.close(fd)


if __name__ == '__main__':
    try:
        main()
    except (KeyError, OSError, ValueError) as error:
        raise SystemExit(f'checkout observation failed: {error}') from None
