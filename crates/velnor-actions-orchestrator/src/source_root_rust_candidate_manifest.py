"""RootRust candidate source observation over the shared OriginalFS engine."""

import os

from source_intent_cold_common import ColdSourceIntent
from source_root_rust_candidate_context import FreshRootRustCandidateContext, _CONTEXT_SEAL
from source_root_rust_candidate_recipe import _LEAVES


def _require_context(context):
    if (type(context) is not FreshRootRustCandidateContext
            or getattr(context, '_seal', None) is not _CONTEXT_SEAL):
        raise ColdSourceIntent('root_candidate_manifest_context_authority')
    context.require_current()


def inventory_candidate_root(context):
    """Return canonical bytes from one sealed candidate observation."""
    _require_context(context)
    if tuple(context.roots) != _LEAVES:
        raise ColdSourceIntent('root_candidate_manifest_roots_changed')
    descriptor = context.root_descriptor
    try:
        from source_archive_inventory_original import _original_inventory

        result = _original_inventory(context.root, _LEAVES, descriptor)
        context.require_current()
        return result.canonical_bytes
    finally:
        os.close(descriptor)
