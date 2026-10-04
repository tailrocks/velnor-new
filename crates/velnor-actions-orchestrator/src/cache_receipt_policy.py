"""Source-bound public receipt policy. No policy may come from cache bytes."""
from dataclasses import MISSING, dataclass, fields
import re

from cache_receipt_common import ColdReceipt

_POLICY_AUTHORITY = object()
_COMPILED_POLICY_CAPSULE = None
_LAYOUT_FIELDS = ("schema", "sdk_paths", "payload_roots", "optional_roots", "payload_indices",
                  "evidence_index", "evidence_root", "evidence_files")
_LAYOUT_ARRAYS = ("sdk_paths", "payload_roots", "optional_roots", "payload_indices", "evidence_files")
_EVIDENCE_FILES = ("manifest.json", "predicate.json", "bundle.sigstore.json")


@dataclass(frozen=True)
class _TransportLayout:
    schema: int
    sdk_paths: tuple
    payload_roots: tuple
    optional_roots: tuple
    payload_indices: tuple
    evidence_index: int
    evidence_root: str
    evidence_files: tuple

    def record(self):
        return {name: getattr(self, name) for name in _LAYOUT_FIELDS}


def _freeze_layout(record):
    if type(record) is not dict or set(record) != set(_LAYOUT_FIELDS):
        raise ColdReceipt("receipt_transport_layout")
    frozen = dict(record)
    for name in _LAYOUT_ARRAYS:
        if type(frozen[name]) not in (list, tuple):
            raise ColdReceipt("receipt_transport_layout")
        frozen[name] = tuple(frozen[name])
    return _TransportLayout(**frozen)


def _require_layout(policy):
    layout = policy.transport_layout
    if type(layout) is not _TransportLayout:
        raise ColdReceipt("receipt_transport_layout")
    roots, optional = policy.allowed_roots, policy.optional_roots
    if (type(layout.schema) is not int or layout.schema != 1
            or layout.payload_roots != roots or layout.optional_roots != optional
            or not 0 < len(roots) <= 32
            or len(set(roots)) != len(roots) or len(set(optional)) != len(optional)):
        raise ColdReceipt("receipt_transport_layout")
    for root in roots:
        if (re.fullmatch(r"[A-Za-z0-9/._-]+", root) is None or len(root) > 4096
                or any(part in ("", ".", "..") for part in root.split("/"))
                or root == "cache-receipts" or root.startswith("cache-receipts/")):
            raise ColdReceipt("receipt_transport_root")
    if any(left != right and right.startswith(left + "/") for left in roots for right in roots):
        raise ColdReceipt("receipt_transport_roots_overlap")
    if (any(type(index) is not int for index in layout.payload_indices)
            or layout.payload_indices != tuple(range(len(roots)))
            or type(layout.evidence_index) is not int or layout.evidence_index != len(roots)
            or not isinstance(layout.evidence_root, str)
            or re.fullmatch(r"cache-receipts/[0-9a-f]{64}", layout.evidence_root) is None
            or layout.evidence_files != _EVIDENCE_FILES):
        raise ColdReceipt("receipt_transport_indices")
    paths = tuple("${{ runner.temp }}/velnor/" + root for root in (*roots, layout.evidence_root))
    if layout.sdk_paths != paths or len("\n".join(paths).encode()) > 32768:
        raise ColdReceipt("receipt_transport_paths")


@dataclass(frozen=True, init=False)
class ReceiptPolicy:
    signer_uri: str
    signer_digest: str
    caller_uri: str
    caller_digest: str
    repository: str
    repository_id: str
    source_ref: str
    source_sha: str
    role: str
    cache_key: str
    descriptor_sha256: str
    helper_sha256: str
    catalog_sha256: str
    policy_sha256: str
    predicate_type: str
    producer_job_name: str
    allowed_roots: tuple[str, ...]
    gh_sha256: str
    trusted_root_sha256: str
    pure_callee_qualified: bool = False
    protected_default_push_qualified: bool = False
    public_attestation_qualified: bool = False
    optional_roots: tuple[str, ...] = ()
    transport_layout: object = None

    def __init__(self, authority=None, **record):
        if authority is not _POLICY_AUTHORITY:
            raise ColdReceipt("receipt_policy_authority")
        admitted = {field.name for field in fields(type(self))}
        if set(record) - admitted:
            raise ColdReceipt("receipt_policy_record")
        for field in fields(type(self)):
            value = record.get(field.name, field.default)
            if value is MISSING:
                raise ColdReceipt("receipt_policy_record")
            if field.name == "transport_layout":
                value = _freeze_layout(value)
            object.__setattr__(self, field.name, value)
        object.__setattr__(self, "_seal", _POLICY_AUTHORITY)

    def source_record(self):
        if getattr(self, "_seal", None) is not _POLICY_AUTHORITY:
            raise ColdReceipt("receipt_policy_authority")
        record = {field.name: getattr(self, field.name) for field in fields(type(self))}
        record["transport_layout"] = self.transport_layout.record()
        return record

    def require_qualified(self):
        if getattr(self, "_seal", None) is not _POLICY_AUTHORITY:
            raise ColdReceipt("receipt_policy_authority")
        booleans = ("pure_callee_qualified", "protected_default_push_qualified",
                    "public_attestation_qualified")
        if any(type(getattr(self, name)) is not bool for name in booleans):
            raise ColdReceipt("receipt_policy_record")
        if any(not isinstance(value, str) or len(value) > 8192
               for name, value in self.source_record().items()
               if name not in booleans + ("allowed_roots", "optional_roots", "transport_layout")):
            raise ColdReceipt("receipt_policy_record")
        if any(type(roots) is not tuple or len(roots) > 256
               or any(not isinstance(root, str) or len(root) > 4096 for root in roots)
               for roots in (self.allowed_roots, self.optional_roots)):
            raise ColdReceipt("receipt_policy_record")
        if not (self.pure_callee_qualified and self.protected_default_push_qualified):
            raise ColdReceipt("producer_policy_unqualified")
        if not self.public_attestation_qualified:
            raise ColdReceipt("attestation_platform_unqualified")
        hashes = (self.gh_sha256, self.trusted_root_sha256, self.descriptor_sha256,
                  self.helper_sha256, self.catalog_sha256, self.policy_sha256)
        if any(not hex_digest(value, 64) for value in hashes):
            raise ColdReceipt("qualified_digest_missing")
        if any(not hex_digest(value, 40) for value in
               (self.signer_digest, self.caller_digest, self.source_sha)):
            raise ColdReceipt("qualified_source_missing")
        if re.fullmatch(r"[1-9][0-9]*", self.repository_id) is None:
            raise ColdReceipt("repository_id_missing")
        if (not self.source_ref.startswith("refs/heads/") or len(self.source_ref) <= 11
                or any(ord(character) <= 32 or character in "~^:?*[\\" for character in self.source_ref)
                or ".." in self.source_ref or "@{" in self.source_ref
                or self.source_ref.endswith(("/", ".", ".lock")) or "//" in self.source_ref):
            raise ColdReceipt("default_ref_missing")
        if re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", self.repository) is None:
            raise ColdReceipt("repository_identity_missing")
        repository_uri = "https://github.com/" + self.repository
        caller_pattern = (re.escape(repository_uri) + r"/\.github/workflows/[A-Za-z0-9_.-]+\.ya?ml@"
                          + re.escape(self.source_ref))
        if (re.fullmatch(caller_pattern, self.caller_uri) is None
                or self.caller_digest != self.source_sha):
            raise ColdReceipt("caller_source_unbound")
        signer_pattern = (r"https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/\.github/workflows/"
                          r"[A-Za-z0-9_.-]+\.ya?ml@" + re.escape(self.signer_digest))
        if re.fullmatch(signer_pattern, self.signer_uri) is None:
            raise ColdReceipt("immutable_signer_missing")
        if (any(root not in self.allowed_roots for root in self.optional_roots)
                or any(root not in ("cargo/.crates.toml", "cargo/.crates2.json")
                       for root in self.optional_roots)):
            raise ColdReceipt("optional_root_unqualified")
        _require_layout(self)


def hex_digest(value, length):
    return isinstance(value, str) and len(value) == length and all(
        character in "0123456789abcdef" for character in value)


def qualified_policy(descriptor):
    """Hosted/source qualification is incomplete; there is no admitted recipe.

    Generation replaces this source-bound factory with a closed compiled recipe
    only after independent callee, protected-source, gh and root qualification.
    There is deliberately no JSON/environment policy loader.

    Capsule producer_helper_closure_sha256 covers the previously qualified
    producer records, arguments, environment and source hashes; callee_commit_sha
    is its immutable reusable-workflow Git commit. Neither hashes this consumer
    module/commit. The capsule supplements the outer consumer transport source
    guard; it never replaces that independently verified source identity.
    """
    capsule = _COMPILED_POLICY_CAPSULE
    if capsule is None:
        raise ColdReceipt("producer_policy_unqualified")
    keys = {"descriptor_sha256", "producer_helper_closure_sha256", "callee_commit_sha", "policy"}
    if not isinstance(capsule, dict) or set(capsule) != keys or not isinstance(capsule["policy"], dict):
        raise ColdReceipt("receipt_policy_capsule")
    record = capsule["policy"]
    if (capsule["descriptor_sha256"] != descriptor
            or record.get("descriptor_sha256") != descriptor
            or capsule["producer_helper_closure_sha256"] != record.get("helper_sha256")
            or capsule["callee_commit_sha"] != record.get("signer_digest")):
        raise ColdReceipt("receipt_policy_capsule_identity")
    policy = ReceiptPolicy(_POLICY_AUTHORITY, **record)
    policy.require_qualified()
    return policy
