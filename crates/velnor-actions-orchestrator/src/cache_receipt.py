"""Pinned-gh public receipt verification. Failure always selects cold state."""
import hashlib
import os
from dataclasses import dataclass

from cache_receipt_virtual import (
    _AuthenticatedManifest, _WITNESS_AUTHORITY, inventory_receipt_quarantine, read_receipt_evidence,
)
from cache_receipt_manifest import canonical, inventory_exact_roots
from cache_receipt_common import ColdReceipt, secure_read, strict_json
from cache_receipt_policy import ReceiptPolicy

_VERIFIED = object()
_FULL_AUTHORITY = object()
_QUARANTINE_AUTHORITY = object()
_FULL_ROOTS = ("mise", "rustup", "cargo/bin", "cargo/.crates.toml", "cargo/.crates2.json")
_FULL_OPTIONAL = ("cargo/.crates.toml", "cargo/.crates2.json")


def _require_policy(policy):
    if type(policy) is not ReceiptPolicy:
        raise ColdReceipt("receipt_policy_authority")
    policy.require_qualified()


@dataclass(frozen=True)
class _GhVerified:
    value: object
    seal: object


@dataclass(frozen=True)
class _ReceiptObservation:
    manifest_sha256: str
    run_id: int
    run_attempt: int
    predicate_bytes: bytes


def _verify(gh, bundle_bytes, data, policy):
    from cache_receipt_gh import QualifiedGh
    if not isinstance(gh, QualifiedGh):
        raise ColdReceipt("verifier_unqualified")
    _require_policy(policy)
    value = gh.verify(bundle_bytes, data, policy)
    return _GhVerified(value, _VERIFIED)


def _certificate(certificate, policy):
    expected = {"buildSignerURI": policy.signer_uri, "buildSignerDigest": policy.signer_digest,
                "subjectAlternativeName": policy.signer_uri,
                "sourceRepositoryURI": "https://github.com/" + policy.repository,
                "sourceRepositoryDigest": policy.source_sha, "sourceRepositoryRef": policy.source_ref,
                "sourceRepositoryIdentifier": policy.repository_id,
                "buildConfigURI": policy.caller_uri, "buildConfigDigest": policy.caller_digest,
                "issuer": "https://token.actions.githubusercontent.com", "buildTrigger": "push",
                "runnerEnvironment": "github-hosted"}
    if not isinstance(certificate, dict) or "extensions" in certificate:
        raise ColdReceipt("certificate_shape")
    if any(certificate.get(key) != value for key, value in expected.items()):
        raise ColdReceipt("certificate_identity")
    visibility = certificate.get("sourceRepositoryVisibilityAtSigning")
    if visibility != "public" or not policy.public_attestation_qualified:
        raise ColdReceipt("attestation_platform_unqualified")


def _predicate(statement, data, policy):
    if not isinstance(statement, dict) or set(statement) != {"_type", "subject", "predicateType", "predicate"}:
        raise ColdReceipt("statement_shape")
    digest = hashlib.sha256(data).hexdigest()
    subject = [{"name": "manifest.json", "digest": {"sha256": digest}}]
    if (statement["_type"] != "https://in-toto.io/Statement/v1"
            or statement["predicateType"] != policy.predicate_type or statement["subject"] != subject):
        raise ColdReceipt("statement_subject")
    predicate = statement["predicate"]
    fields = ("role", "cache_key", "descriptor_sha256", "helper_sha256", "catalog_sha256",
              "policy_sha256", "repository_id", "source_sha")
    expected = {key: getattr(policy, key) for key in fields}
    expected.update(schema=1, manifest_sha256=digest)
    if (not isinstance(predicate, dict) or set(predicate) != set(expected) | {"run_id", "run_attempt"}
            or any(type(predicate[key]) is not type(value) or predicate[key] != value
                   for key, value in expected.items())):
        raise ColdReceipt("predicate_identity")
    if any(type(predicate[key]) is not int or not 0 < predicate[key] < 2**64
           for key in ("run_id", "run_attempt")):
        raise ColdReceipt("predicate_invocation")
    return predicate


def _attempt(gh, policy, predicate):
    run, attempt = predicate["run_id"], predicate["run_attempt"]
    endpoint = f"repos/{policy.repository}/actions/runs/{run}/attempts/{attempt}"
    metadata = gh.api(endpoint)
    expected = {"id": run, "run_attempt": attempt, "head_sha": policy.source_sha,
                "head_branch": policy.source_ref.removeprefix("refs/heads/"), "event": "push",
                "status": "completed", "conclusion": "success"}
    if not isinstance(metadata, dict) or any(
            type(metadata.get(key)) is not type(value) or metadata.get(key) != value
            for key, value in expected.items()):
        raise ColdReceipt("run_attempt_identity")
    repository_id = metadata.get("repository", {}).get("id")
    if type(repository_id) is not int or str(repository_id) != policy.repository_id:
        raise ColdReceipt("run_repository_identity")
    pages = gh.api(endpoint + "/jobs?per_page=100", paginate=True)
    if not isinstance(pages, list) or not pages or len(pages) > 1000:
        raise ColdReceipt("jobs_unavailable")
    jobs = []
    for page in pages:
        if (not isinstance(page, dict) or type(page.get("total_count")) is not int
                or not isinstance(page.get("jobs"), list) or len(page["jobs"]) > 100):
            raise ColdReceipt("jobs_shape")
        jobs.extend(page["jobs"])
    if any(not isinstance(job, dict) or type(job.get("id")) is not int
           or job["id"] <= 0 for job in jobs):
        raise ColdReceipt("jobs_shape")
    totals = {page.get("total_count") for page in pages}
    if totals != {len(jobs)} or len({job.get("id") for job in jobs}) != len(jobs):
        raise ColdReceipt("jobs_incomplete")
    selected = [job for job in jobs if job.get("name") == policy.producer_job_name]
    if len(selected) != 1:
        raise ColdReceipt("producer_job_missing_or_ambiguous")
    job = selected[0]
    if (type(job.get("run_id")) is not int or type(job.get("run_attempt")) is not int
            or job.get("run_id") != run or job.get("run_attempt") != attempt
            or job.get("status") != "completed" or job.get("conclusion") != "success"):
        raise ColdReceipt("producer_job_unsuccessful")


def _admit(verified, data, policy, gh):
    _require_policy(policy)
    if not isinstance(verified, _GhVerified) or verified.seal is not _VERIFIED:
        raise ColdReceipt("unverified_bundle")
    value = verified.value
    if not isinstance(value, list) or len(value) != 1:
        raise ColdReceipt("verification_count")
    try:
        result = value[0]["verificationResult"]
        certificate = result["signature"]["certificate"]
        _certificate(certificate, policy)
        predicate = _predicate(result["statement"], data, policy)
        invocation = (f"https://github.com/{policy.repository}/actions/runs/"
                      f"{predicate['run_id']}/attempts/{predicate['run_attempt']}")
        if certificate.get("runInvocationURI") != invocation:
            raise ColdReceipt("certificate_invocation")
        _attempt(gh, policy, predicate)
    except (KeyError, TypeError, AttributeError) as error:
        raise ColdReceipt("verified_evidence_shape") from error
    return _ReceiptObservation(hashlib.sha256(data).hexdigest(), predicate["run_id"],
                               predicate["run_attempt"], canonical(predicate))


def verify_payload(quarantine, gh, policy):
    """Admit numbered payload using only the sealed final evidence root.

    Signed inventory excludes circular receipt bytes. Source policy selects
    every numbered root; archive metadata and caller bundle paths cannot.
    Caller retains exclusive ownership through materialization.
    """
    _require_policy(policy)
    quarantine = os.fspath(quarantine)
    try:
        evidence = read_receipt_evidence(quarantine, policy)
        data = evidence["manifest.json"]
        verified = _verify(gh, evidence["bundle.sigstore.json"], data, policy)
        admitted = _admit(verified, data, policy, gh)
        predicate = evidence["predicate.json"]
        if canonical(strict_json(predicate)) != predicate or predicate != admitted.predicate_bytes:
            raise ColdReceipt("quarantine_predicate_mismatch")
        witness = _AuthenticatedManifest(_WITNESS_AUTHORITY, data, policy)
        observed = inventory_receipt_quarantine(quarantine, policy, witness)
        if observed != (data, evidence):
            raise ColdReceipt("payload_changed_after_verification")
        if inventory_receipt_quarantine(quarantine, policy, witness) != observed:
            raise ColdReceipt("payload_changed_after_verification")
        return VerifiedQuarantinePayload(_QUARANTINE_AUTHORITY, quarantine, policy, witness,
                                         evidence, admitted)
    except (OSError, RecursionError) as error:
        raise ColdReceipt("receipt_evidence_unavailable") from error



class VerifiedQuarantinePayload:
    """Private same-process materialization grant for one current quarantine."""
    __slots__ = ("_quarantine", "_policy", "_witness", "_evidence", "_observation", "_seal")

    def __init__(self, authority, quarantine, policy, witness, evidence, observation):
        if authority is not _QUARANTINE_AUTHORITY:
            raise ColdReceipt("quarantine_payload_authority")
        witness.require(policy)
        for name, value in (("_quarantine", quarantine), ("_policy", policy),
                            ("_witness", witness), ("_evidence", tuple(evidence.items())),
                            ("_observation", observation), ("_seal", _QUARANTINE_AUTHORITY)):
            object.__setattr__(self, name, value)

    def __setattr__(self, _name, _value):
        raise ColdReceipt("quarantine_payload_immutable")

    @property
    def quarantine(self):
        return self._quarantine

    @property
    def manifest_bytes(self):
        return self._witness.require(self._policy)

    @property
    def logical_roots(self):
        return self._policy.allowed_roots

    @property
    def optional_roots(self):
        return self._policy.optional_roots

    @property
    def manifest_sha256(self):
        return hashlib.sha256(self.manifest_bytes).hexdigest()

    @property
    def evidence_sha256(self):
        record = {name: hashlib.sha256(data).hexdigest() for name, data in self._evidence}
        return hashlib.sha256(canonical(record)).hexdigest()

    @property
    def run_id(self):
        return self._observation.run_id

    @property
    def run_attempt(self):
        return self._observation.run_attempt

    def require_current(self):
        if getattr(self, "_seal", None) is not _QUARANTINE_AUTHORITY:
            raise ColdReceipt("quarantine_payload_authority")
        _require_policy(self._policy)
        data, evidence = inventory_receipt_quarantine(self._quarantine, self._policy, self._witness)
        if data != self.manifest_bytes or tuple(evidence.items()) != self._evidence:
            raise ColdReceipt("quarantine_payload_changed")

def _prove_manifest(data, refresh, bundle, gh, policy):
    verified = _verify(gh, secure_read(bundle), data, policy)
    admitted = _admit(verified, data, policy, gh)
    if refresh() != data:
        raise ColdReceipt("payload_changed_after_verification")
    return admitted


class VerifiedFullToolPayload:
    """Same-process proof bound to one live Full tool inventory, never JSON."""
    __slots__ = ("_root", "_manifest", "_role", "_descriptor", "_seal")

    def __init__(self, authority, root, data, policy):
        if authority is not _FULL_AUTHORITY:
            raise ColdReceipt("full_tool_authority")
        object.__setattr__(self, "_root", root)
        object.__setattr__(self, "_manifest", data)
        object.__setattr__(self, "_role", policy.role)
        object.__setattr__(self, "_descriptor", policy.descriptor_sha256)
        object.__setattr__(self, "_seal", _FULL_AUTHORITY)

    def __setattr__(self, _name, _value):
        raise ColdReceipt("full_tool_grant_immutable")

    @property
    def root(self):
        return self._root

    @property
    def manifest_bytes(self):
        return self._manifest

    @property
    def role(self):
        return self._role

    @property
    def descriptor_sha256(self):
        return self._descriptor

    def require_current(self):
        if self._seal is not _FULL_AUTHORITY:
            raise ColdReceipt("full_tool_authority")
        if inventory_exact_roots(self._root, _FULL_ROOTS, _FULL_OPTIONAL) != self._manifest:
            raise ColdReceipt("full_tool_payload_changed")


def verify_live_payload(payloadroot, bundle, qualifiedgh, compiledpolicy):
    """Sole live tool grant factory: verify all bytes before Rustup execution."""
    _require_policy(compiledpolicy)
    if (compiledpolicy.role != "tool-full" or compiledpolicy.allowed_roots != _FULL_ROOTS
            or compiledpolicy.optional_roots != _FULL_OPTIONAL):
        raise ColdReceipt("full_tool_recipe_unqualified")
    try:
        refresh = lambda: inventory_exact_roots(payloadroot, _FULL_ROOTS, _FULL_OPTIONAL)
        data = refresh()
        _prove_manifest(data, refresh, bundle, qualifiedgh, compiledpolicy)
        return VerifiedFullToolPayload(_FULL_AUTHORITY, payloadroot, data, compiledpolicy)
    except (OSError, RecursionError) as error:
        raise ColdReceipt("receipt_evidence_unavailable") from error
