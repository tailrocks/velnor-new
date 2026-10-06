"""Dedicated owned original-archive verifier; source selection grants no runtime rights.

The source owner must issue the opaque role after actual distribution, Foundation
and kernel qualification. No adopted role exists yet. Missing authority is checked
before inspecting RUNNER_TEMP, touching files, or executing a process.
"""
import hashlib
import os
import selectors
import signal
import stat
import subprocess
import sys
import time

_COMPILED_NATIVE_VERIFIER_SOURCE_ROLE = None
_SDK_SEAL = object()
_INSTALLATION_SEAL = object()
_TOOL_SEAL = object()
_ROLE_SEAL = object()


class NativeVerifierUnavailable(RuntimeError):
    pass


def _require(condition, reason):
    if not condition:
        raise NativeVerifierUnavailable(reason)


class _NativeVerifierSourceRole:
    """Opaque source-issued issuer interface, never deserialized receipt authority.

    There is intentionally no Python data constructor. The source factory must
    issue this object from its actual owned distribution and runtime capabilities.
    It binds the complete fresh installer and full installed/runtime observations,
    rather than authorizing an executable from a version string or source digest.
    """
    __slots__ = ('_seal', '_installer', '_runtime', '_distribution', '_identity')

    def __init__(self):
        raise NativeVerifierUnavailable('native_verifier_owned_source_role_unavailable')

    def __setattr__(self, _name, _value):
        raise NativeVerifierUnavailable('native_verifier_source_role_immutable')

    def require_current(self):
        _require(type(self) is _NativeVerifierSourceRole and
                 getattr(self, '_seal', None) is _ROLE_SEAL and
                 self is _COMPILED_NATIVE_VERIFIER_SOURCE_ROLE,
                 'native_verifier_source_role_authority')
        # These objects remain owner-held opaque capabilities. They are not JSON
        # receipts, caller callbacks, shell strings or path-authority substitutes.
        _require(sys.platform == 'linux' and os.uname().machine == 'x86_64',
                 'native_verifier_actual_host')
        self._distribution.require_current()
        self._runtime.require_current()
        self._installer.require_current()
        _require(self._distribution.identity == self._identity,
                 'native_verifier_distribution_identity_changed')

    def install_fresh(self):
        self.require_current()
        installed = self._installer.install_native_verifier()
        _require(type(installed) is _NativeVerifierInstallation and
                 getattr(installed, '_seal', None) is _INSTALLATION_SEAL,
                 'native_verifier_installation_origin')
        installed.require_current()
        self.require_current()
        return installed


class _NativeVerifierInstallation:
    """Actual fresh install and exclusive live root, held by the qualified installer.

    Issuance belongs to the exact whole source installer. Public paths, manifests
    and matching digests cannot construct this object.
    """
    __slots__ = ('_role', '_root', '_descriptor', '_identity', '_manifest',
                 '_tool_hashes', '_seal')

    def __init__(self):
        raise NativeVerifierUnavailable('native_verifier_installer_origin_required')

    def __setattr__(self, _name, _value):
        raise NativeVerifierUnavailable('native_verifier_installation_immutable')

    def require_current(self):
        _require(getattr(self, '_seal', None) is _INSTALLATION_SEAL and
                 type(self._role) is _NativeVerifierSourceRole,
                 'native_verifier_installation_authority')
        self._role.require_current()
        _require(self._descriptor is not None, 'native_verifier_installation_closed')
        _require(set(self._tool_hashes) == {'bin/velnor-cargo-verifier',
                     'verify_original_archives_linux.py'}, 'native_verifier_tool_inventory')
        current, identity = _inventory(self._root)
        _require(identity == self._identity and _identity(os.fstat(self._descriptor)) == identity,
                 'native_verifier_live_root_changed')
        _require(current == self._manifest, 'native_verifier_full_closure_changed')
        for relative, expected in self._tool_hashes.items():
            _require(type(expected) is str and len(expected) == 64 and
                     all(char in '0123456789abcdef' for char in expected),
                     'native_verifier_tool_digest_shape')
            _require(_regular_hash(self._root + '/' + relative) == expected,
                     'native_verifier_installed_tool_changed')
        self._role.require_current()

    def close(self):
        descriptor = self._descriptor
        object.__setattr__(self, '_descriptor', None)
        if descriptor is not None:
            os.close(descriptor)


class NativeVerifierSdk:
    """Dedicated owned verifier capability; never stock Cargo or publisher authority."""
    __slots__ = ('_installation', '_seal')

    def __init__(self, installation, *, _seal=None):
        _require(_seal is _SDK_SEAL and type(installation) is _NativeVerifierInstallation,
                 'native_verifier_sdk_origin')
        installation.require_current()
        object.__setattr__(self, '_installation', installation)
        object.__setattr__(self, '_seal', _seal)

    def __setattr__(self, _name, _value):
        raise NativeVerifierUnavailable('native_verifier_sdk_immutable')

    def require_current(self):
        _require(getattr(self, '_seal', None) is _SDK_SEAL,
                 'native_verifier_sdk_authority')
        self._installation.require_current()

    def installed_tool(self):
        self.require_current()
        return _NativeVerifierInstalledTool(self, _seal=_TOOL_SEAL)

    @property
    def host(self):
        self.require_current()
        return 'x86_64-unknown-linux-gnu'

    def _materialize_verification_inputs(self, operand):
        session, _path = _original_session(operand, self, operation='materialize')
        role = self._installation._role
        role.require_current()
        role._installer.materialize_verification_inputs(session, self._installation)
        role.require_current()
        self.require_current()

    def prepare_original_archives(self, root):
        """Prepare native state through an authenticated original session operand."""
        session, path = _original_session(root, self, operation='prepare')
        proof = self.installed_tool()
        proof.require_current()
        argv = [proof.path, 'prepare-original-archives', '--session',
                path + '/verification-session.json']
        status = _capture_owned(argv, _prepare_environment(path), path,
                                self, session, operation='prepare')
        _require(status == 0, 'native_verifier_preparation_failed')
        # Native session.prepared and registry/work bytes are output data. The
        # genuine session owner must validate and seal them; status grants nothing.
        session._native_preparation_complete()
        session.require_current()
        self.require_current()

    def verify_original_archives(self, root):
        """Fixed source wrapper owns all bwrap arguments, mounts and seccomp rules."""
        session, path = _original_session(root, self, operation='verify')
        installation = self._installation
        installation._role._runtime.require_current()
        helper = installation._root + '/verify_original_archives_linux.py'
        python = installation._role._runtime.python_path
        argv = [python, '-I', '-S', '-B', helper, path]
        status = _capture_owned(argv, {'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8'},
                                '/', self, session, operation='verify')
        session.require_current()
        self.require_current()
        _require(status == 0, 'native_verifier_verification_failed')

    def close(self):
        self._installation.close()


class _NativeVerifierInstalledTool:
    """Private installed proof for the dedicated owned binary only."""
    __slots__ = ('_sdk', '_seal')

    def __init__(self, sdk, *, _seal=None):
        _require(_seal is _TOOL_SEAL and type(sdk) is NativeVerifierSdk,
                 'native_verifier_tool_origin')
        sdk.require_current()
        object.__setattr__(self, '_sdk', sdk)
        object.__setattr__(self, '_seal', _seal)

    def __setattr__(self, _name, _value):
        raise NativeVerifierUnavailable('native_verifier_tool_immutable')

    def require_current(self):
        _require(getattr(self, '_seal', None) is _TOOL_SEAL,
                 'native_verifier_tool_authority')
        self._sdk.require_current()

    @property
    def path(self):
        self.require_current()
        return self._sdk._installation._root + '/bin/velnor-cargo-verifier'

    @property
    def sha256(self):
        self.require_current()
        return self._sdk._installation._tool_hashes['bin/velnor-cargo-verifier']


def _identity(info):
    return info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid


def _inventory(path):
    from source_archive_inventory_fs import root_descriptor
    from source_archive_inventory_original import _original_inventory
    descriptor = root_descriptor(path)
    try:
        info = os.fstat(descriptor)
        _require(info.st_uid == os.geteuid() and not stat.S_IMODE(info.st_mode) & 0o022,
                 'native_verifier_root_owner')
        roots = tuple(sorted(os.listdir(descriptor)))
        manifest = _original_inventory(path, roots, descriptor).canonical_bytes
        return manifest, _identity(info)
    finally:
        os.close(descriptor)


def _regular_hash(path):
    from source_archive_inventory_fs import root_descriptor
    parent = root_descriptor(os.path.dirname(path))
    try:
        descriptor = os.open(os.path.basename(path), os.O_RDONLY | os.O_NOFOLLOW |
                             os.O_NONBLOCK, dir_fd=parent)
    finally:
        os.close(parent)
    try:
        before = os.fstat(descriptor)
        _require(stat.S_ISREG(before.st_mode) and not before.st_mode & 0o7022,
                 'native_verifier_tool_file')
        digest, size = hashlib.sha256(), 0
        with os.fdopen(descriptor, 'rb', closefd=False) as stream:
            while chunk := stream.read(1024 * 1024):
                size += len(chunk)
                _require(size <= 1024 * 1024 * 1024, 'native_verifier_tool_limit')
                digest.update(chunk)
        after = os.fstat(descriptor)
        stable = lambda item: (item.st_dev, item.st_ino, item.st_mode, item.st_size,
                               item.st_mtime_ns, item.st_ctime_ns)
        _require(stable(before) == stable(after), 'native_verifier_tool_mutated')
        return digest.hexdigest()
    finally:
        os.close(descriptor)


def _original_session(root, sdk, operation):
    try:
        session_type = _OriginalArchiveVerificationSession
        validator = validate_original_archive_verification_session
    except NameError as error:
        raise NativeVerifierUnavailable('native_verifier_fixed_session_owner_unavailable') from error
    _require(type(root) is session_type, 'native_verifier_original_session_authority')
    validator(root)
    root.require_native_verifier_sdk(sdk, operation)
    path = root.root_path
    _require(type(path) is str and path.startswith('/'), 'native_verifier_session_root')
    root.require_current()
    sdk.require_current()
    return root, path


def _prepare_environment(path):
    return {'CARGO_HOME': path + '/cargo-home', 'RUSTUP_HOME': path + '/toolchain',
            'CARGO': path + '/toolchain/bin/cargo', 'RUSTC': path + '/toolchain/bin/rustc',
            'PATH': path + '/toolchain/bin', 'HOME': path + '/preparation-work/home',
            'TMPDIR': path + '/preparation-work/tmp', 'LANG': 'C.UTF-8',
            'LC_ALL': 'C.UTF-8', 'TZ': 'UTC'}


def _capture_owned(argv, environment, cwd, sdk, session, operation):
    sdk.require_current()
    _original_session(session, sdk, operation)
    selector, process = selectors.DefaultSelector(), None
    deadline, total = time.monotonic() + 900, 0
    try:
        process = subprocess.Popen(argv, cwd=cwd, env=environment, shell=False,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            close_fds=True, start_new_session=True)
        process.stdin.close()
        for stream in (process.stdout, process.stderr):
            selector.register(stream, selectors.EVENT_READ)
        while selector.get_map():
            _require(time.monotonic() < deadline, 'native_verifier_process_timeout')
            for key, _event in selector.select(0.1):
                data = os.read(key.fileobj.fileno(), 65536)
                if not data:
                    selector.unregister(key.fileobj)
                    continue
                total += len(data)
                _require(total <= 8 * 1024 * 1024, 'native_verifier_log_limit')
        status = process.wait(timeout=max(0.01, deadline - time.monotonic()))
        _stop_process_group(process)
        sdk.require_current()
        if operation == 'verify':
            _original_session(session, sdk, operation)
        return status
    finally:
        if process is not None:
            _stop_process_group(process)
            process.wait()
            for stream in (process.stdin, process.stdout, process.stderr):
                if stream is not None:
                    stream.close()
        selector.close()


def _stop_process_group(process):
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def load_native_verifier_sdk():
    """Zero caller inputs; missing actual adopted issuer stops before any effects."""
    role = _COMPILED_NATIVE_VERIFIER_SOURCE_ROLE
    _require(type(role) is _NativeVerifierSourceRole,
             'native_verifier_authority_unavailable: owned-source/build/archive/full-installed-closure/first-step-Foundation/Linux-kernel receipts required')
    role.require_current()
    installation = role.install_fresh()
    try:
        return NativeVerifierSdk(installation, _seal=_SDK_SEAL)
    except BaseException:
        installation.close()
        raise
