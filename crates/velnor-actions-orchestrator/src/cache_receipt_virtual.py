"""Inventory numbered quarantine roots in the compiled logical namespace.

Archive observations never select mappings. Structural prefixes are derived
from immutable roots and have no claimed authenticated mode or metadata.
"""
import os
import posixpath
import re

from cache_receipt_manifest import (
    MAX_ENTRIES, _root_descriptor, canonical, relative,
)
from cache_receipt_common import ColdReceipt, _absolute, secure_read, strict_json


def _inventory(quarantine, logical_roots, optional_roots, evidence_index, witness=None):
    """Canonical observation with verifier-owned authenticated target projection."""
    from source_archive_inventory import _observe_numbered, _finish
    from source_archive_inventory_common import InventoryError, prefixes
    errors = {"payload_roots_unqualified": "quarantine_roots_unqualified",
              "payload_roots_overlap": "quarantine_roots_overlap",
              "payload_optional_root_invalid": "quarantine_optional_roots",
              "payload_inventory_limit": "quarantine_inventory_limit",
              "payload_manifest_limit": "quarantine_manifest_limit"}
    try:
        entries, size, roots, optional = _observe_numbered(
            quarantine, logical_roots, optional_roots, evidence_index)
        if witness is not None:
            _project_targets(entries, roots, witness)
        return _finish(entries, size, roots, optional, prefixes(roots)).canonical_bytes
    except InventoryError as error:
        raise ColdReceipt(errors.get(str(error), str(error))) from error


def inventory_quarantine(quarantine, logical_roots, optional_roots=()):
    """Unauthenticated inventory utility; this projection never admits bytes."""
    return _inventory(quarantine, logical_roots, optional_roots, None)


def _evidence(quarantine, layout):
    physical = os.path.join(quarantine, "roots", str(layout.evidence_index))
    try:
        descriptor = _root_descriptor(physical)
        try:
            if set(os.listdir(descriptor)) != set(layout.evidence_files):
                raise ColdReceipt("quarantine_evidence_files")
            evidence = {name: secure_read(os.path.join(physical, name))
                        for name in layout.evidence_files}
            if set(os.listdir(descriptor)) != set(layout.evidence_files):
                raise ColdReceipt("quarantine_evidence_changed")
            return evidence
        finally:
            os.close(descriptor)
    except (OSError, RecursionError) as error:
        raise ColdReceipt("quarantine_evidence_unavailable") from error



_WITNESS_AUTHORITY = object()


class _AuthenticatedManifest:
    """Private verifier-minted subject; JSON and caller flags cannot mint it."""
    __slots__ = ("_data", "_policy", "_seal")

    def __init__(self, authority, data, policy):
        if authority is not _WITNESS_AUTHORITY or type(data) is not bytes:
            raise ColdReceipt("receipt_manifest_authority")
        _signed_entries(data)
        object.__setattr__(self, "_data", data)
        object.__setattr__(self, "_policy", policy)
        object.__setattr__(self, "_seal", _WITNESS_AUTHORITY)

    def __setattr__(self, _name, _value):
        raise ColdReceipt("receipt_manifest_immutable")

    def require(self, policy):
        if (getattr(self, "_seal", None) is not _WITNESS_AUTHORITY
                or getattr(self, "_policy", None) is not policy):
            raise ColdReceipt("receipt_manifest_authority")
        return self._data


def _signed_entries(data):
    record = strict_json(data)
    if (not isinstance(record, dict) or set(record) != {"schema", "entries"}
            or type(record["schema"]) is not int or record["schema"] != 2
            or not isinstance(record["entries"], list)
            or not 0 < len(record["entries"]) <= MAX_ENTRIES or canonical(record) != data):
        raise ColdReceipt("receipt_manifest_shape")
    result = {}
    fields = {"path", "mode", "kind", "sha256", "target"}
    for entry in record["entries"]:
        if (not isinstance(entry, dict) or set(entry) != fields
                or not isinstance(entry["path"], str) or entry["path"] in result
                or type(entry["mode"]) is not int or not 0 <= entry["mode"] <= 0o777
                or entry["kind"] not in ("file", "directory", "symlink", "missing")):
            raise ColdReceipt("receipt_manifest_entry")
        relative(entry["path"])
        if ((entry["kind"] == "symlink") != isinstance(entry["target"], str)
                or (entry["kind"] != "symlink" and entry["target"] is not None)):
            raise ColdReceipt("receipt_manifest_entry")
        if entry["kind"] == "file":
            from cache_receipt_policy import hex_digest
            if not hex_digest(entry["sha256"], 64):
                raise ColdReceipt("receipt_manifest_entry")
        elif entry["sha256"] is not None:
            raise ColdReceipt("receipt_manifest_entry")
        result[entry["path"]] = entry
    return result


def _numbered(path, roots):
    from source_archive_inventory_common import InventoryError, numbered
    try:
        return numbered(path, roots)
    except InventoryError as error:
        raise ColdReceipt(str(error)) from error


def _project_targets(entries, roots, witness):
    from source_archive_inventory_common import InventoryError, direct_target, prefixes
    expected = _signed_entries(witness._data)
    observed = {entry["path"]: entry for entry in entries}
    for entry in entries:
        if entry["kind"] != "symlink":
            continue
        signed = expected.get(entry["path"])
        if signed is None or signed["kind"] != "symlink":
            raise ColdReceipt("quarantine_manifest_mismatch")
        target = signed["target"]
        if (not target or target.startswith("/") or "\\" in target or "\0" in target
                or re.match(r"[A-Za-z]:", target) is not None or len(os.fsencode(target)) > 4096):
            raise ColdReceipt("payload_symlink_escape")
        try:
            resolved = direct_target(entry["path"], target, observed, prefixes(roots))
        except InventoryError as error:
            raise ColdReceipt(str(error)) from error
        transformed = posixpath.relpath(_numbered(resolved, roots),
                                       posixpath.dirname(_numbered(entry["path"], roots)))
        if entry["target"] != transformed:
            raise ColdReceipt("quarantine_symlink_transform")
        entry["target"] = target


def read_receipt_evidence(quarantine, policy):
    """Bounded untrusted bytes; the verifier must authenticate the subject first."""
    from cache_receipt_policy import ReceiptPolicy
    if type(policy) is not ReceiptPolicy:
        raise ColdReceipt("receipt_policy_authority")
    policy.require_qualified()
    _absolute(quarantine)
    return _evidence(quarantine, policy.transport_layout)


def inventory_receipt_quarantine(quarantine, policy, witness):
    """Observe exact source layout using only authenticated raw-target witnesses."""
    evidence = read_receipt_evidence(quarantine, policy)
    if type(witness) is not _AuthenticatedManifest:
        raise ColdReceipt("receipt_manifest_authority")
    expected = witness.require(policy)
    if evidence["manifest.json"] != expected:
        raise ColdReceipt("quarantine_manifest_mismatch")
    data = _inventory(quarantine, policy.allowed_roots, policy.optional_roots,
                      policy.transport_layout.evidence_index, witness)
    if data != expected:
        raise ColdReceipt("quarantine_manifest_mismatch")
    return data, evidence
