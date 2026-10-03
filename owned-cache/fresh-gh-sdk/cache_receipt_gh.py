"""Closed GitHub verifier bootstrap from compiler-owned distribution authority."""
import fcntl
import hashlib
import os
import re
import selectors
import stat
import subprocess
import tempfile
import time

from cache_receipt_common import ColdReceipt, strict_json

_LIMIT = 64 * 1024 * 1024
_OUTPUT_LIMIT = 8 * 1024 * 1024
_ERROR_LIMIT = 256 * 1024
_TIMEOUT = 45
_AUTHORITY = object()
# Replaced only by the qualified compiled source owner, never runtime input.
_COMPILED_GH_DISTRIBUTION = None


def _digest(value):
    return isinstance(value, str) and re.fullmatch(r"[a-f0-9]{64}", value) is not None


def _root(path):
    if not isinstance(path, str) or not path.startswith('/'):
        raise ColdReceipt('gh_temporary_root')
    parts = path.split('/')[1:]
    if any(part in ('', '.', '..') for part in parts):
        raise ColdReceipt('gh_temporary_root')
    descriptor = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in parts:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        info = os.fstat(descriptor)
        if info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ColdReceipt('gh_temporary_root')
    except OSError as error:
        raise ColdReceipt('gh_temporary_root') from error
    finally:
        os.close(descriptor)


def _regular_source(path):
    if not isinstance(path, str) or not path.startswith('/'):
        raise ColdReceipt('gh_binary_path')
    parts = path.split('/')[1:]
    if any(part in ('', '.', '..') for part in parts):
        raise ColdReceipt('gh_binary_path')
    parent = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
            os.close(parent)
            parent = child
        descriptor = os.open(parts[-1], os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW, dir_fd=parent)
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            os.close(descriptor)
            raise ColdReceipt('gh_binary_shape')
        return descriptor
    except OSError as error:
        raise ColdReceipt('gh_binary_path') from error
    finally:
        os.close(parent)


def _seal_binary(path, record):
    if not (os.uname().sysname == 'Linux' and record.get('tool') == 'gh'
            and record.get('version') == '2.102.0'
            and record.get('machine') == os.uname().machine
            and _digest(record.get('binary_sha256'))
            and _digest(record.get('qualification_sha256'))):
        raise ColdReceipt('gh_host_unqualified')
    source = _regular_source(path)
    sealed = None
    try:
        info = os.fstat(source)
        if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= _LIMIT:
            raise ColdReceipt('gh_binary_shape')
        sealed = os.memfd_create('velnor-qualified-gh', os.MFD_ALLOW_SEALING)
        os.fchmod(sealed, 0o500)
        digest = hashlib.sha256()
        total = 0
        while chunk := os.read(source, 1024 * 1024):
            total += len(chunk)
            if total > _LIMIT:
                raise ColdReceipt('gh_binary_size')
            digest.update(chunk)
            remaining = memoryview(chunk)
            while remaining:
                written = os.write(sealed, remaining)
                if written <= 0:
                    raise ColdReceipt('gh_binary_write')
                remaining = remaining[written:]
        if total != info.st_size or digest.hexdigest() != record['binary_sha256']:
            raise ColdReceipt('gh_binary_digest')
        seals = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
        fcntl.fcntl(sealed, fcntl.F_ADD_SEALS, seals)
        if fcntl.fcntl(sealed, fcntl.F_GET_SEALS) != seals:
            raise ColdReceipt('gh_binary_seal')
        return sealed
    except BaseException:
        if sealed is not None:
            os.close(sealed)
        raise
    finally:
        os.close(source)


def _environment(home):
    return {'PATH': '/usr/bin:/bin', 'HOME': home,
            'GH_CONFIG_DIR': home + '/gh', 'XDG_CONFIG_HOME': home + '/config',
            'XDG_CACHE_HOME': home + '/cache', 'TMPDIR': home,
            'GH_PROMPT_DISABLED': '1', 'GH_PAGER': '/bin/cat',
            'GH_TELEMETRY': 'disabled',
            'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8'}


def _run(descriptor, arguments, home):
    command = ['/proc/self/fd/' + str(descriptor), *arguments]
    process = subprocess.Popen(command, env=_environment(home), cwd=home,
                               pass_fds=(descriptor,), stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, stdin=subprocess.DEVNULL)
    chunks = {process.stdout: bytearray(), process.stderr: bytearray()}
    selector = selectors.DefaultSelector()
    started = time.monotonic()
    try:
        for stream in chunks:
            selector.register(stream, selectors.EVENT_READ)
        while selector.get_map():
            if time.monotonic() - started > _TIMEOUT:
                raise ColdReceipt('gh_timeout')
            for key, _ in selector.select(0.1):
                data = os.read(key.fileobj.fileno(), 65536)
                if not data:
                    selector.unregister(key.fileobj)
                    continue
                chunks[key.fileobj].extend(data)
                limit = _OUTPUT_LIMIT if key.fileobj is process.stdout else _ERROR_LIMIT
                if len(chunks[key.fileobj]) > limit:
                    raise ColdReceipt('gh_output_size')
        remaining = max(0.01, _TIMEOUT - (time.monotonic() - started))
        if process.wait(timeout=remaining) != 0:
            raise ColdReceipt('gh_verification_failed')
        try:
            return strict_json(chunks[process.stdout])
        except (ValueError, UnicodeError) as error:
            raise ColdReceipt('gh_output_json') from error
    except subprocess.TimeoutExpired as error:
        raise ColdReceipt('gh_timeout') from error
    finally:
        if process.poll() is None:
            process.kill()
        process.wait()
        selector.close()
        process.stdout.close()
        process.stderr.close()


class QualifiedGh:
    """Source-owner issued sealed executable; never created from cache records."""
    def __init__(self, authority, descriptor, record, root_bytes, temporary_root, repository):
        if authority is not _AUTHORITY:
            raise ColdReceipt('gh_authority')
        self._descriptor = descriptor
        self._record = record
        self._root_bytes = root_bytes
        self._temporary_root = temporary_root
        self._repository = repository

    def close(self):
        if self._descriptor is not None:
            os.close(self._descriptor)
            self._descriptor = None

    def verify(self, bundle_bytes, manifest_bytes, policy):
        policy.require_qualified()
        if (policy.repository != self._repository
                or self._record['binary_sha256'] != policy.gh_sha256
                or hashlib.sha256(self._root_bytes).hexdigest() != policy.trusted_root_sha256):
            raise ColdReceipt('gh_policy_binding')
        if len(bundle_bytes) > _OUTPUT_LIMIT or len(manifest_bytes) > _OUTPUT_LIMIT:
            raise ColdReceipt('gh_document_size')
        with tempfile.TemporaryDirectory(prefix='velnor-receipt-', dir=self._temporary_root) as home:
            for name, data in (('bundle.json', bundle_bytes), ('manifest.json', manifest_bytes),
                               ('root.json', self._root_bytes)):
                descriptor = os.open(home + '/' + name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                with os.fdopen(descriptor, 'wb') as output:
                    output.write(data)
            arguments = ['attestation', 'verify', home + '/manifest.json', '--bundle',
                         home + '/bundle.json', '--repo', policy.repository,
                         '--cert-identity', policy.signer_uri,
                         '--signer-digest', policy.signer_digest, '--source-ref', policy.source_ref,
                         '--source-digest', policy.source_sha, '--predicate-type', policy.predicate_type,
                         '--cert-oidc-issuer', 'https://token.actions.githubusercontent.com',
                         '--deny-self-hosted-runners', '--custom-trusted-root', home + '/root.json',
                         '--format', 'json']
            return _run(self._descriptor, arguments, home)

    def api(self, endpoint, paginate=False):
        from cache_receipt_api import PublicReceiptApi
        return PublicReceiptApi(self._repository).api(endpoint, paginate=paginate)



def qualified_gh(path, root_bytes, temporary_root, repository):
    """The source-bound factory embeds the compiled distribution projection.

    There is no JSON/env projection loader. The factory must select the record
    from QualifiedDistribution, never a restored manifest or caller argument.
    """
    if (re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository) is None
            or not isinstance(root_bytes, bytes) or not 0 < len(root_bytes) <= 65536):
        raise ColdReceipt('gh_qualified_input')
    if _COMPILED_GH_DISTRIBUTION is None:
        raise ColdReceipt('gh_distribution_unqualified')
    _root(temporary_root)
    descriptor = _seal_binary(path, _COMPILED_GH_DISTRIBUTION)
    return QualifiedGh(_AUTHORITY, descriptor, dict(_COMPILED_GH_DISTRIBUTION), root_bytes,
                       temporary_root, repository)
