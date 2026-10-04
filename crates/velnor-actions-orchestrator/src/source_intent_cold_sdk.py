"""Independent cold SourceIntent SDK; cached receipts cannot construct it."""
import hashlib
from types import MappingProxyType

from source_intent_cold_common import ColdSourceIntent
from source_intent_cold_context import FreshColdInstallationContext, _from_completed_installation
from source_intent_cold_install import _entries, _regular_hash, execute_cold_installation, observe_cold_installation
from source_intent_cold_recipe import _record_digest, _require_qualification

_SDK_SEAL = object()
_TOOL_SEAL = object()
_GENESIS_FIELDS = frozenset({
    'schema', 'purpose', 'phases', 'host', 'installer_source_identity',
    'mise_qualification_sha256', 'foundation_qualification_sha256',
    'installed_manifest_sha256', 'tool_identities', 'installed_obligations',
    'recipe_sha256',
})


def _require_genesis(context, manifest, genesis):
    """Bind summary genesis to this witness and its canonical manifest."""
    context.require_current()
    if type(manifest) is not bytes or type(genesis) is not dict:
        raise ColdSourceIntent('cold_sdk_genesis_binding')
    witness = context._witness
    recipe = dict(witness._recipe)
    expected = {
        'schema': 1,
        'purpose': recipe['purpose'],
        'phases': [dict(item) for item in witness._phases],
        'host': recipe['host'],
        'installer_source_identity': recipe['installer_source_identity'],
        'mise_qualification_sha256': recipe['mise_qualification_sha256'],
        'foundation_qualification_sha256': context._profile['qualification_sha256'],
        'installed_manifest_sha256': hashlib.sha256(manifest).hexdigest(),
        'recipe_sha256': _record_digest(recipe),
    }
    if set(genesis) != _GENESIS_FIELDS or any(
            genesis[name] != value for name, value in expected.items()):
        raise ColdSourceIntent('cold_sdk_genesis_binding')


def _require_tool_bindings(context, manifest, tools):
    if type(tools) is not dict or set(tools) != {'cargo', 'rustc', 'rustdoc'}:
        raise ColdSourceIntent('cold_sdk_tool_inventory')
    entries, root = _entries(manifest), context.root
    for tool in ('cargo', 'rustc', 'rustdoc'):
        relative = 'rustup-home/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/' + tool
        entry = entries.get(relative)
        if (type(entry) is not dict or entry.get('kind') != 'file'
                or tools[tool] != (root + '/' + relative, entry.get('sha256'))
                or _regular_hash(root + '/' + relative) != entry.get('sha256')):
            raise ColdSourceIntent('cold_sdk_tool_binding')


class ColdSourceIntentSdk:
    """Actual fresh installer genesis and immutable original tool image."""
    __slots__ = ('_context', '_manifest', '_tools', '_genesis', '_seal')

    def __init__(self, context, manifest, tools, genesis, *, _seal=None):
        if _seal is not _SDK_SEAL or type(context) is not FreshColdInstallationContext:
            raise ColdSourceIntent('cold_sdk_authority')
        context.require_current()
        _require_genesis(context, manifest, genesis)
        _require_qualification(dict(context._witness._recipe), genesis)
        _require_tool_bindings(context, manifest, tools)
        object.__setattr__(self, '_context', context)
        object.__setattr__(self, '_manifest', manifest)
        object.__setattr__(self, '_tools', MappingProxyType(dict(tools)))
        object.__setattr__(self, '_genesis', MappingProxyType(dict(genesis)))
        object.__setattr__(self, '_seal', _seal)
        self.require_current()

    def __setattr__(self, _name, _value):
        raise ColdSourceIntent('cold_sdk_immutable')

    def require_current(self):
        from source_intent_cold_manifest import source_original_inventory
        if getattr(self, '_seal', None) is not _SDK_SEAL:
            raise ColdSourceIntent('cold_sdk_authority')
        self._context.require_current()
        if source_original_inventory(self._context).canonical_bytes != self._manifest:
            raise ColdSourceIntent('cold_sdk_postgrant_mutation')
        for path, digest in self._tools.values():
            if _regular_hash(path) != digest:
                raise ColdSourceIntent('cold_sdk_postgrant_executable_mutation')

    @property
    def rust_version(self):
        self.require_current()
        return '1.98.1'

    @property
    def host(self):
        self.require_current()
        return 'x86_64-unknown-linux-gnu'

    def require_policy(self, rust_version, actual_host):
        self.require_current()
        if rust_version != '1.98.1' or actual_host != 'x86_64-unknown-linux-gnu':
            raise ColdSourceIntent('cold_sdk_policy')

    def installed_tool(self, tool):
        self.require_current()
        if tool not in ('cargo', 'rustc', 'rustdoc'):
            raise ColdSourceIntent('cold_sdk_tool')
        return ColdSourceIntentInstalledTool(self, tool, _seal=_TOOL_SEAL)

    def genesis(self):
        """Diagnostic identities and hashes only; original raw attrs stay private."""
        self.require_current()
        return {key: self._genesis[key] for key in ('schema', 'purpose', 'host',
                'installer_source_identity', 'mise_qualification_sha256',
                'installed_manifest_sha256', 'recipe_sha256')}


class ColdSourceIntentInstalledTool:
    """Private stock tool proof; never an owned Cargo verifier capability."""
    __slots__ = ('_sdk', '_tool', '_seal')

    def __init__(self, sdk, tool, *, _seal=None):
        if _seal is not _TOOL_SEAL or type(sdk) is not ColdSourceIntentSdk:
            raise ColdSourceIntent('cold_sdk_tool_authority')
        sdk.require_current()
        if tool not in sdk._tools:
            raise ColdSourceIntent('cold_sdk_tool')
        object.__setattr__(self, '_sdk', sdk)
        object.__setattr__(self, '_tool', tool)
        object.__setattr__(self, '_seal', _seal)

    def __setattr__(self, _name, _value):
        raise ColdSourceIntent('cold_sdk_tool_immutable')

    def require_current(self):
        if getattr(self, '_seal', None) is not _TOOL_SEAL:
            raise ColdSourceIntent('cold_sdk_tool_authority')
        self._sdk.require_current()

    @property
    def path(self):
        self.require_current()
        return self._sdk._tools[self._tool][0]

    @property
    def sha256(self):
        self.require_current()
        return self._sdk._tools[self._tool][1]


def load_source_intent_sdk():
    """Zero caller inputs; source owner fixes installer and qualification capsule."""
    witness = execute_cold_installation()
    context = _from_completed_installation(witness)
    try:
        manifest, tools, genesis = observe_cold_installation(witness, context)
        return ColdSourceIntentSdk(context, manifest, tools, genesis, _seal=_SDK_SEAL)
    except BaseException:
        context.close()
        raise
