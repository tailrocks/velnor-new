"""Neutral Rustup metadata launch, granted only by the live receipt verifier.

The source owner freezes descriptor and complete receipt-policy literals. Neither a
serialized receipt nor an externally supplied descriptor grants execution.
"""
import os
import hashlib
import json
import selectors
import subprocess
import time
from types import MappingProxyType

from cache_receipt_common import ColdReceipt, strict_json

_COMPILED_RUSTUP_METADATA = None
_COMPILED_RECEIPT_POLICY_SHA256 = None
_LAUNCH_SEAL = object()
_FULL_ROOTS = ('mise', 'rustup', 'cargo/bin', 'cargo/.crates.toml', 'cargo/.crates2.json')
_QUALIFICATION = {'commit': 'd95a37b6ab92cc1e455d1576039333c97ca3e2c5',
                  'tree': 'a346236d560c33eaaadced018b78581e3b45d245'}


def _descriptor():
    value = _COMPILED_RUSTUP_METADATA
    fields = {'schema', 'role', 'host', 'manager_version', 'manager_sha256',
              'qualification', 'toolchain', 'manager', 'proxy', 'settings', 'cargo', 'rustc', 'rustdoc'}
    if (not isinstance(value, dict) or set(value) != fields
            or type(value['schema']) is not int or value['schema'] != 1
            or value['qualification'] != _QUALIFICATION
            or value['manager_version'] != '1.29.1'):
        raise ColdReceipt('rustup_launch_source_unqualified')
    roles = {'root-linux': ('x86_64-unknown-linux-gnu', '1.98.1',
              'dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71'),
             'desktop-mac': ('aarch64-apple-darwin', '1.99.0',
              'ec1b9233e7f72990ecd8e62063fa7f6c3dfc2bec8e97f88bff165f9100ac696a'),
             'desktop-source-mac': ('aarch64-apple-darwin', '1.99.0',
              'ec1b9233e7f72990ecd8e62063fa7f6c3dfc2bec8e97f88bff165f9100ac696a'),
             'release-mac': ('aarch64-apple-darwin', '1.98.1',
              'ec1b9233e7f72990ecd8e62063fa7f6c3dfc2bec8e97f88bff165f9100ac696a')}
    selected = roles.get(value['role']) if isinstance(value['role'], str) else None
    if selected is None:
        raise ColdReceipt('rustup_launch_role')
    host, compiler, digest = selected
    toolchain = compiler + '-' + host
    expected = {'host': host, 'manager_sha256': digest, 'toolchain': toolchain,
                'manager': 'cargo/bin/rustup', 'proxy': 'cargo/bin/cargo',
                'settings': 'rustup/settings.toml',
                'cargo': 'rustup/toolchains/' + toolchain + '/bin/cargo',
                'rustc': 'rustup/toolchains/' + toolchain + '/bin/rustc',
                'rustdoc': 'rustup/toolchains/' + toolchain + '/bin/rustdoc'}
    if any(value[key] != item for key, item in expected.items()):
        raise ColdReceipt('rustup_launch_descriptor')
    return dict(value)


def _environment(environment, root):
    if not isinstance(environment, dict) or len(environment) > 4096:
        raise ColdReceipt('rustup_launch_environment')
    result = dict(environment)
    for key, value in result.items():
        if (not isinstance(key, str) or not isinstance(value, str) or not key
                or '=' in key or '\0' in key + value or len(key) > 4096 or len(value) > 131072
                or key.startswith(('LD_', 'DYLD_'))):
            raise ColdReceipt('rustup_launch_environment')
    if (result.get('CARGO_HOME') != root + '/cargo'
            or result.get('RUSTUP_HOME') != root + '/rustup'):
        raise ColdReceipt('rustup_launch_roots')
    result['RUSTUP_AUTO_INSTALL'] = '0'
    return result


def _receipt_policy(policy):
    from cache_receipt_policy import ReceiptPolicy, hex_digest
    expected = _COMPILED_RECEIPT_POLICY_SHA256
    if type(policy) is not ReceiptPolicy or not hex_digest(expected, 64):
        raise ColdReceipt('rustup_launch_policy_unqualified')
    policy.require_qualified()
    # Closed source helper freezes the complete actual policy, including all
    # signer, recipe, platform and source claims; qualification booleans alone
    # cannot authorize a caller-selected receipt policy.
    encoded = (json.dumps(policy.source_record(), sort_keys=True, separators=(',', ':'),
                          ensure_ascii=True, allow_nan=False) + '\n').encode('ascii')
    if hashlib.sha256(encoded).hexdigest() != expected:
        raise ColdReceipt('rustup_launch_policy_mismatch')


def _inventory(manifest, descriptor):
    value = strict_json(manifest)
    if (not isinstance(value, dict) or set(value) != {'schema', 'entries'}
            or type(value['schema']) is not int or value['schema'] != 2
            or not isinstance(value['entries'], list) or len(value['entries']) > 100000
            or any(not isinstance(entry, dict) or not isinstance(entry.get('path'), str)
                   for entry in value['entries'])):
        raise ColdReceipt('rustup_launch_manifest')
    entries = {entry['path']: entry for entry in value['entries']}
    if len(entries) != len(value['entries']):
        raise ColdReceipt('rustup_launch_manifest')
    for field in ('manager', 'proxy', 'settings', 'cargo', 'rustc', 'rustdoc'):
        entry = entries.get(descriptor[field])
        if (not isinstance(entry, dict) or entry.get('kind') != 'file'
                or type(entry.get('mode')) is not int
                or not isinstance(entry.get('sha256'), str)):
            raise ColdReceipt('rustup_launch_manifest_missing')
        if field != 'settings' and not entry.get('mode', 0) & 0o111:
            raise ColdReceipt('rustup_launch_not_executable')
        if field in ('manager', 'proxy') and entry.get('sha256') != descriptor['manager_sha256']:
            raise ColdReceipt('rustup_launch_manager_digest')


def _observe_child(command, environment, cwd, limit, timeout):
    started = time.monotonic_ns()
    process = subprocess.Popen(command, cwd=cwd, env=environment,
        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    selector, output = selectors.DefaultSelector(), bytearray()
    deadline = time.monotonic() + timeout
    try:
        selector.register(process.stdout, selectors.EVENT_READ)
        while selector.get_map():
            if time.monotonic() >= deadline:
                raise ColdReceipt('rustup_launch_owner_timeout')
            for key, _ in selector.select(0.1):
                data = os.read(key.fileobj.fileno(), min(65536, limit + 1 - len(output)))
                if not data:
                    selector.unregister(key.fileobj)
                    continue
                output.extend(data)
                if len(output) > limit:
                    raise ColdReceipt('rustup_launch_owner_output_limit')
        status = process.wait(timeout=max(0.01, deadline - time.monotonic()))
        return bytes(output), status, time.monotonic_ns() - started
    finally:
        if process.poll() is None:
            process.kill()
        process.wait()
        selector.close()
        process.stdout.close()


def _which(command, environment, cwd):
    output, status, _elapsed = _observe_child(command, environment, cwd, 4096, 10)
    if status != 0:
        raise ColdReceipt('rustup_launch_owner_unavailable')
    return output


def _metadata_arguments(arguments):
    if (not isinstance(arguments, (list, tuple)) or len(arguments) > 4096
            or any(not isinstance(item, str) or '\0' in item or len(item) > 4096
                   for item in arguments)):
        raise ColdReceipt('rustup_launch_arguments')
    values, index = list(arguments), 0
    while index < len(values) and values[index] != 'metadata':
        flag = values[index].split('=', 1)[0]
        if flag not in ('-C', '--directory', '--config', '-Z'):
            raise ColdReceipt('rustup_launch_arguments')
        index += 1 if '=' in values[index] else 2
    if index >= len(values):
        raise ColdReceipt('rustup_launch_arguments')
    index += 1
    toggles = {'--locked', '--offline', '--all-features', '--no-default-features'}
    fields = {'--format-version', '--manifest-path', '--features', '--filter-platform'}
    seen = {}
    while index < len(values):
        item = values[index]
        flag, separator, inline = item.partition('=')
        if flag in toggles and not separator:
            seen[flag] = True
            index += 1
        elif flag in fields:
            if not separator:
                index += 1
                if index >= len(values):
                    raise ColdReceipt('rustup_launch_arguments')
                inline = values[index]
            if not inline or (flag == '--format-version' and inline != '1'):
                raise ColdReceipt('rustup_launch_arguments')
            seen[flag] = inline
            index += 1
        else:
            raise ColdReceipt('rustup_launch_arguments')
    if not {'--locked', '--offline', '--format-version'} <= set(seen):
        raise ColdReceipt('rustup_launch_arguments')
    return values


class VerifiedRustupMetadataLaunch:
    """Same-process launch capability; deliberately has no JSON constructor."""
    __slots__ = ('_payload', '_descriptor', '_environment', '_cwd', '_policy', '_seal')

    def __init__(self, payload, descriptor, environment, cwd, *, _seal=None, _policy=None):
        from cache_receipt import VerifiedFullToolPayload
        if _seal is not _LAUNCH_SEAL or type(payload) is not VerifiedFullToolPayload:
            raise ColdReceipt('rustup_launch_unverified')
        payload.require_current()
        object.__setattr__(self, '_payload', payload)
        object.__setattr__(self, '_descriptor', MappingProxyType(descriptor))
        object.__setattr__(self, '_environment', MappingProxyType(environment))
        object.__setattr__(self, '_cwd', cwd)
        object.__setattr__(self, '_policy', _policy)
        object.__setattr__(self, '_seal', _seal)

    def __setattr__(self, _name, _value):
        raise ColdReceipt('rustup_launch_immutable')

    def installed_tool(self, tool):
        """Actual owner-resolved path and authenticated digest; no JSON grant."""
        if self._seal is not _LAUNCH_SEAL or tool not in ('cargo', 'rustc', 'rustdoc'):
            raise ColdReceipt('rustup_launch_unverified')
        self._payload.require_current()
        path = self._descriptor[tool]
        entries = strict_json(self._payload.manifest_bytes)['entries']
        entry = next(item for item in entries if item['path'] == path)
        return self._payload.root + '/' + path, entry['sha256']

    def command(self, arguments):
        if self._seal is not _LAUNCH_SEAL:
            raise ColdReceipt('rustup_launch_unverified')
        self._payload.require_current()
        arguments = _metadata_arguments(arguments)
        root = self._payload.root
        return ([root + '/' + self._descriptor['proxy'],
                 '+' + self._descriptor['toolchain'], *arguments],
                dict(self._environment), self._cwd)

    def observe_metadata(self, arguments, *, original_program, original_arguments):
        """Execute fresh metadata here; export observations, never authority.

        Native code receives bounded stdout/status/wall only from the actual
        source-qualified helper process. A JSON record is never a launch grant.
        """
        command, environment, cwd = self.command(arguments)
        if (original_program != command[0] or not isinstance(original_arguments, (list, tuple))
                or len(original_arguments) > 4096
                or any(not isinstance(item, str) or '\0' in item or len(item) > 4096
                       for item in original_arguments)):
            raise ColdReceipt('rustup_launch_original_selection')
        selector = original_arguments[0] if original_arguments else ''
        if selector.startswith('+'):
            if selector != command[1]:
                raise ColdReceipt('rustup_launch_original_selection')
        else:
            self._payload.require_current()
            try:
                selected = _which([self._payload.root + '/' + self._descriptor['manager'],
                                   'which', 'cargo'], environment, cwd)
            except (OSError, subprocess.TimeoutExpired) as error:
                raise ColdReceipt('rustup_launch_original_selection') from error
            expected = (self._payload.root + '/' + self._descriptor['cargo'] + '\n').encode('utf-8')
            if selected != expected:
                raise ColdReceipt('rustup_launch_original_selection')
            self._payload.require_current()
        try:
            output, status, elapsed = _observe_child(command, environment, cwd,
                                                     16 * 1024 * 1024, 30)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise ColdReceipt('rustup_launch_metadata_unavailable') from error
        self._payload.require_current()
        return output, status, elapsed


def verify_rustup_metadata_launch(payload_root, bundle, gh, policy, environment, cwd):
    """Authenticate complete live full payload before executing the manager.

    Caller exclusively owns admitted roots until the metadata child finishes.
    This function is called inside the authenticated source helper, never from
    an archive-supplied module or a ReceiptContext deserializer.
    """
    from cache_receipt import VerifiedFullToolPayload, verify_live_payload
    descriptor = _descriptor()
    _receipt_policy(policy)
    if policy.role != 'tool-full' or tuple(policy.allowed_roots) != _FULL_ROOTS:
        raise ColdReceipt('rustup_launch_domain')
    if (not isinstance(cwd, str) or not os.path.isabs(cwd)
            or os.path.realpath(cwd) != cwd or not os.path.isdir(cwd)):
        raise ColdReceipt('rustup_launch_cwd')
    payload = verify_live_payload(payload_root, bundle, gh, policy)
    if type(payload) is not VerifiedFullToolPayload:
        raise ColdReceipt('rustup_launch_unverified')
    child_environment = _environment(environment, payload.root)
    _inventory(payload.manifest_bytes, descriptor)
    payload.require_current()
    for tool in ('cargo', 'rustc', 'rustdoc'):
        try:
            actual = _which(
                [payload.root + '/' + descriptor['manager'], 'which', '--toolchain',
                 descriptor['toolchain'], tool], child_environment, cwd)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise ColdReceipt('rustup_launch_owner_unavailable') from error
        expected = (payload.root + '/' + descriptor[tool] + '\n').encode('utf-8')
        if actual != expected:
            raise ColdReceipt('rustup_launch_owner_mismatch')
        payload.require_current()
    return VerifiedRustupMetadataLaunch(payload, descriptor, child_environment, cwd,
                                        _seal=_LAUNCH_SEAL, _policy=policy)
