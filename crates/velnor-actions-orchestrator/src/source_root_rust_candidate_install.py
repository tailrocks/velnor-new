"""Fixed Rustup candidate genesis. Never constructs a SourceIntent SDK."""
import os
import stat
import hashlib
import tempfile
from types import MappingProxyType

from source_archive_inventory_fs import root_descriptor
from source_intent_cold_common import ColdSourceIntent
from source_intent_cold_foundation import FreshPreparationFoundation, require_preparation_foundation
from source_intent_cold_install import _regular_hash, _binary_descriptor, _descriptor_hash
from source_root_rust_candidate_process import observe_candidate_child
from source_root_rust_candidate_recipe import _compiled_candidate_recipe, _candidate_environment

_CANDIDATE_INSTALLATION_SEAL = object()


def _observe_candidate_verified(path, expected_sha256, arguments, environment, root, limit):
    descriptor = _binary_descriptor(path)
    try:
        if _descriptor_hash(descriptor) != expected_sha256:
            raise ColdSourceIntent('root_candidate_spawn_executable_changed')
        return observe_candidate_child([path, *arguments], environment, root, limit, 10,
                                       executable_descriptor=descriptor)
    finally:
        os.close(descriptor)


def _run_stage(stage, environment):
    with tempfile.TemporaryDirectory(prefix='velnor-root-rust-stage-',
                                     dir=environment['RUNNER_TEMP']) as directory:
        path = directory + '/stage.sh'
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(stage['source'].encode('utf-8'))
        with open(path, 'rb') as stream:
            actual = hashlib.sha256(stream.read()).hexdigest()
        if actual != stage['source_sha256']:
            raise ColdSourceIntent('root_candidate_stage_changed')
        output, status, wall = observe_candidate_child(
            ['/bin/bash', '--noprofile', '--norc', '-p', path, *stage['arguments']],
            dict(environment, HOME=directory), environment['RUNNER_TEMP'], 8 * 1024 * 1024, 900)
        if status != 0:
            raise ColdSourceIntent('root_candidate_stage_failed:' + stage['name'])
        return {'name': stage['name'], 'source_sha256': actual, 'exit_code': status,
                'wall_ns': wall, 'stdout_sha256': hashlib.sha256(output).hexdigest()}


def _require_empty_candidate(root):
    descriptor = root_descriptor(root)
    try:
        info = os.fstat(descriptor)
        if info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) & 0o022:
            raise ColdSourceIntent('root_candidate_owner')
        if os.listdir(descriptor):
            raise ColdSourceIntent('root_candidate_not_empty')
    finally:
        os.close(descriptor)


def _require_phase_layout(root, acquired):
    descriptor = root_descriptor(root)
    leaves = {'cargo-home', 'rustup-home', 'rustup-bootstrap', 'native-dist'}
    if not acquired:
        leaves.add('manager-bin')
    try:
        if set(os.listdir(descriptor)) != leaves:
            raise ColdSourceIntent('root_candidate_phase_roots')
        for leaf in sorted(leaves):
            child = os.open(leaf, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=descriptor)
            try:
                info = os.fstat(child)
                if info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) & 0o022:
                    raise ColdSourceIntent('root_candidate_leaf_owner')
                if leaf == 'rustup-bootstrap' and os.listdir(child) != ['rustup-init']:
                    raise ColdSourceIntent('root_candidate_bootstrap_entries')
                if acquired and leaf == 'rustup-home' and os.listdir(child):
                    raise ColdSourceIntent('root_candidate_early_toolchain')
                if leaf == 'cargo-home' and acquired:
                    if os.listdir(child) != ['bin']:
                        raise ColdSourceIntent('root_candidate_early_cargo')
                    binaries = os.open('bin', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=child)
                    try:
                        if os.listdir(binaries):
                            raise ColdSourceIntent('root_candidate_early_manager')
                    finally:
                        os.close(binaries)
                if leaf == 'manager-bin':
                    if (os.listdir(child) != ['rustup']
                            or not stat.S_ISLNK(os.stat('rustup', dir_fd=child, follow_symlinks=False).st_mode)
                            or os.readlink('rustup', dir_fd=child) != root + '/cargo-home/bin/rustup'):
                        raise ColdSourceIntent('root_candidate_manager_link')
            finally:
                os.close(child)
    finally:
        os.close(descriptor)


class RootRustCandidateInstallationWitness:
    """Private actual waits retain the same pre-Python Foundation issuer."""
    __slots__ = ('_recipe', '_root', '_environment', '_phases', '_foundation', '_seal')

    def __init__(self, recipe, root, environment, phases, foundation, *, _seal=None):
        if (_seal is not _CANDIDATE_INSTALLATION_SEAL
                or type(foundation) is not FreshPreparationFoundation):
            raise ColdSourceIntent('root_candidate_witness_authority')
        foundation.require_candidate_root(root)
        for name, value in (('_recipe', MappingProxyType(recipe)), ('_root', root),
                            ('_environment', MappingProxyType(dict(environment))),
                            ('_phases', tuple(MappingProxyType(item) for item in phases)),
                            ('_foundation', foundation), ('_seal', _seal)):
            object.__setattr__(self, name, value)
        self.require_current()

    def __setattr__(self, _name, _value):
        raise ColdSourceIntent('root_candidate_witness_immutable')

    def require_current(self):
        if getattr(self, '_seal', None) is not _CANDIDATE_INSTALLATION_SEAL:
            raise ColdSourceIntent('root_candidate_witness_authority')
        self._foundation.require_candidate_root(self._root)
        self._foundation.require_acquired_native()
        self._foundation.require_installed_native()
        recipe = _compiled_candidate_recipe()
        root, environment = _candidate_environment(recipe, self._foundation)
        if (recipe != dict(self._recipe) or root != self._root
                or environment != dict(self._environment)):
            raise ColdSourceIntent('root_candidate_source_changed')
        if _regular_hash(root + '/cargo-home/bin/rustup') != recipe['manager_sha256']:
            raise ColdSourceIntent('root_candidate_manager_changed')

    @property
    def root(self):
        self.require_current()
        return self._root


def execute_root_rust_candidate():
    recipe = _compiled_candidate_recipe()
    foundation = require_preparation_foundation('root-linux-compiler-candidate')
    root, environment = _candidate_environment(recipe, foundation)
    foundation.require_candidate_root(root)
    phases = []
    for stage in recipe['stages']:
        foundation.require_candidate_root(root)
        if stage['name'] == 'install':
            foundation.require_acquired_native()
            foundation.require_compiler_source_native()
        phases.append(_run_stage(stage, environment))
        foundation.require_candidate_root(root)
        if stage['name'] == 'clear':
            _require_empty_candidate(root)
        elif stage['name'] == 'acquire':
            if _regular_hash(root + '/rustup-bootstrap/rustup-init') != recipe['manager_sha256']:
                raise ColdSourceIntent('root_candidate_acquired_manager_changed')
            _require_phase_layout(root, acquired=True)
            foundation.require_acquired_native()
        elif _regular_hash(root + '/cargo-home/bin/rustup') != recipe['manager_sha256']:
            raise ColdSourceIntent('root_candidate_manager_changed')
        if stage['name'] == 'install':
            _require_phase_layout(root, acquired=False)
            foundation.require_installed_native()
    return RootRustCandidateInstallationWitness(recipe, root, environment, phases, foundation,
                                                _seal=_CANDIDATE_INSTALLATION_SEAL)
