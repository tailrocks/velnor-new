"""Qualified SourceIntent facade over the sole original-source observer."""

import os

from source_intent_cold_common import ColdSourceIntent
from source_intent_cold_context import FreshColdInstallationContext, _CONTEXT_SEAL
from source_intent_cold_recipe import _LEAVES


def _require_context(context):
    if (type(context) is not FreshColdInstallationContext
            or getattr(context, "_seal", None) is not _CONTEXT_SEAL):
        raise ColdSourceIntent("cold_manifest_context_authority")
    context.require_current()


def source_original_inventory(context):
    """Return the complete original-source observation from a sealed context."""
    _require_context(context)
    if tuple(context.roots) != _LEAVES:
        raise ColdSourceIntent("cold_manifest_roots_changed")
    descriptor = context.root_descriptor
    try:
        from source_archive_inventory_original import _original_inventory

        result = _original_inventory(context.root, _LEAVES, descriptor)
        context.require_current()
        return result
    finally:
        os.close(descriptor)


def inventory_cold_root(context):
    """Return canonical bytes from the qualified fresh-install context."""
    return source_original_inventory(context).canonical_bytes
