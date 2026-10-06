"""Finite source-owned preparation; publication remains unqualified.

Embedded stages come from complete owner reconstruction. Runtime environment,
cache action outputs and writable files carry observations, never warm grants.
"""
import os
import subprocess

from cache_receipt_common import ColdReceipt, _absolute
from cache_receipt_materialize import _capture_source_namespace, _materialize_verified
from cache_receipt_materialize_transaction import MaterializationStop
from cache_receipt import VerifiedQuarantinePayload, verify_payload


class _PublicationMiss:
    __slots__ = ()


class _RejectedReceipt:
    __slots__ = ()


_MISS = _PublicationMiss()
_REJECTED = _RejectedReceipt()


def _qualified_runtime():
    # No hosted publication, source binding or archive projection is qualified.
    # This fixed source decision is independent of caller JSON and cache bytes.
    return _MISS


def _runner_environment():
    temp = os.environ['RUNNER_TEMP']
    _absolute(temp)
    environment = {'RUNNER_TEMP': temp, 'PATH': '/usr/bin:/bin:/usr/sbin:/sbin',
                   'HOME': temp + '/velnor-receipt-control/home',
                   'LC_ALL': 'C', 'LANG': 'C'}
    for name in ('GITHUB_PATH', 'GITHUB_OUTPUT'):
        value = os.environ[name]
        _absolute(value)
        if value == temp + '/velnor' or value.startswith(temp + '/velnor/'):
            raise RuntimeError('receipt_preparation_runner_control_in_payload')
        environment[name] = value
    return environment


def _run_source(stage):
    source, arguments, bindings = stage
    environment = _runner_environment()
    for target, source_name in bindings:
        environment[target] = os.environ[source_name]
    subprocess.run(
        ['/bin/bash', '--noprofile', '--norc', '-p', '-c', source,
         'velnor-receipt-owned-stage', *arguments],
        env=environment, check=True,
    )


def _cold(stages, cleared):
    if not cleared:
        _run_source(stages[0])
    _run_source(stages[1])
    _run_source(stages[2])


def _warm(grant, namespace, stage):
    # Admission has happened. Every later exception is terminal, including a
    # ColdReceipt from a changed quarantine, namespace or live inventory.
    if type(grant) is not VerifiedQuarantinePayload:
        raise MaterializationStop('receipt_preparation_grant_authority')
    _materialize_verified(grant, namespace)
    grant.require_current()
    namespace.require(grant)
    actual = namespace._projection.inventory_live(namespace._path, grant._policy)
    if actual != grant.manifest_bytes:
        raise MaterializationStop('receipt_preparation_live_payload_changed')
    grant.require_current()
    namespace.require(grant)
    _run_source(stage)


def prepare_owned(stages):
    """Fixed source-qualified admission, materialization and continuation."""
    if type(stages) is not tuple or len(stages) != 4:
        raise RuntimeError('receipt_preparation_source_registry')
    context = _qualified_runtime()
    if context is _MISS:
        _cold(stages, False)
        return
    # A future provider must mint real policy/projection capabilities and freshly
    # acquire qualified Gh in control storage outside every payload root.
    namespace = None
    try:
        namespace = _capture_source_namespace()
        _run_source(stages[0])
        try:
            grant = verify_payload(context.quarantine, context.gh, context.policy)
        except ColdReceipt:
            decision = _REJECTED
        else:
            decision = grant
        if decision is _REJECTED:
            _cold(stages, True)
            return
        _warm(decision, namespace, stages[3])
    finally:
        try:
            if namespace is not None:
                namespace.close()
        finally:
            context.gh.close()
