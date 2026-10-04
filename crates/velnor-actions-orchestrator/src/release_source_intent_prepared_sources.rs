//! Unregistered complete Prepared source composer; no qualification or workflow hook.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use velnor_actions_contract::{CompiledSupportSource, canonical_json_str};
use velnor_actions_mise::source_archive_inventory::{InventorySourceProgram, fixed_sources};
use velnor_actions_rust::release_config::{EmitOptions, VersionGroup, emit_release_plz_config};

use crate::OrchestratorError;
use crate::release_emit::release_source_artifact_input::CompiledSourceArtifactInput;
use crate::release_emit::release_steps::JobInputs;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PREFIX: &str = ".github/velnor/";
const MAIN_ORDER: &[&str] = &[
    "release_reconcile_common.py",
    "release_source_validation.py",
    "release_reconcile_cargo.py",
    "release_package_contract.py",
    "release_publish_metadata.py",
    "release_publish_manifest.py",
    "release_forge_publish_verify.py",
    "release_source_tree.py",
    "release_source_snapshot.py",
    "release_source_materialize.py",
    "release_source_artifact_input.py",
    "release_source_intent_context.py",
    "release_source_intent_output.py",
    "release_prepare_notes.py",
    "release_source_intent_descriptor.py",
    "release_source_intent_contract.py",
    "release_source_intent_guard.py",
    "release_source_intent_cargo.py",
    "compiled_source_artifact_input.py",
    "release_source_intent_prepare.py",
];
const LOCAL_MAIN: &[(&str, &str)] = &[
    (
        "release_source_tree.py",
        include_str!("release_source_tree.py"),
    ),
    (
        "release_source_snapshot.py",
        include_str!("release_source_snapshot.py"),
    ),
    (
        "release_source_materialize.py",
        include_str!("release_source_materialize.py"),
    ),
    (
        "release_source_artifact_input.py",
        include_str!("release_source_artifact_input.py"),
    ),
    (
        "release_source_intent_context.py",
        include_str!("release_source_intent_context.py"),
    ),
    (
        "release_source_intent_output.py",
        include_str!("release_source_intent_output.py"),
    ),
    (
        "release_source_intent_descriptor.py",
        include_str!("release_source_intent_descriptor.py"),
    ),
];
const LOCAL_COLD: &[(&str, &str)] = &[
    (
        "source_intent_cold_common",
        include_str!("source_intent_cold_common.py"),
    ),
    (
        "source_intent_cold_compiler",
        include_str!("source_intent_cold_compiler.py"),
    ),
    (
        "source_intent_cold_recipe",
        include_str!("source_intent_cold_recipe.py"),
    ),
    (
        "source_intent_cold_foundation",
        include_str!("source_intent_cold_foundation.py"),
    ),
    (
        "source_intent_cold_context",
        include_str!("source_intent_cold_context.py"),
    ),
    (
        "source_intent_cold_manifest",
        include_str!("source_intent_cold_manifest.py"),
    ),
    (
        "source_intent_cold_process",
        include_str!("source_intent_cold_process.py"),
    ),
    (
        "source_intent_cold_install",
        include_str!("source_intent_cold_install.py"),
    ),
    (
        "source_intent_cold_sdk",
        include_str!("source_intent_cold_sdk.py"),
    ),
];

/// Private generation record; source bytes and transport environment are inseparable.
/// This is not a qualified runtime SDK or a registered execution helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompiledPreparedSource {
    body: String,
    environment: BTreeMap<String, String>,
}

impl CompiledPreparedSource {
    pub(super) fn compile(inputs: &JobInputs<'_>) -> Result<Self, OrchestratorError> {
        let configuration = configuration(inputs)?;
        let input = CompiledSourceArtifactInput::compile(inputs)?;
        Ok(Self {
            body: compose(&input, configuration)?,
            environment: input.environment().clone(),
        })
    }

    pub(super) fn body(&self) -> &str {
        &self.body
    }

    pub(super) fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}

fn compose(
    input: &CompiledSourceArtifactInput,
    configuration: Value,
) -> Result<String, OrchestratorError> {
    let mut main = main_sources()?;
    insert(
        &mut main,
        "compiled_source_artifact_input.py",
        input.runtime_source()?,
    )?;
    let cold = cold_sources()?;
    let capsule = canonical_json_str(&json!({
        "configuration": configuration,
        "main_order": MAIN_ORDER,
        "main_sources": main,
        "cold_order": cold.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        "cold_sources": cold.into_iter().collect::<BTreeMap<_, _>>(),
    }))?
    .replace('$', "\\u0024");
    if capsule.contains("\"\"\"") {
        return Err(invalid("capsule_literal_delimiter"));
    }
    Ok(format!(
        "set -euo pipefail\npython3 -I -S - \"$@\" <<'VELNOR_PREPARED_COMPILED_BODY'\n\
         import json\n_CAPSULE = json.loads(r\"\"\"{capsule}\"\"\")\n\
         {LAUNCHER}\nVELNOR_PREPARED_COMPILED_BODY\n"
    ))
}

fn configuration(inputs: &JobInputs<'_>) -> Result<Value, OrchestratorError> {
    let actual_host = velnor_actions_contract::tool_target_for_runner_label(inputs.label)
        .ok_or_else(|| invalid("actual_host"))?;
    let groups = inputs
        .release
        .version_groups
        .iter()
        .map(|(name, members)| VersionGroup {
            name: name.clone(),
            members: members.clone(),
        })
        .collect::<Vec<_>>();
    let release_config = emit_release_plz_config(
        inputs.selection,
        &EmitOptions {
            tag_pattern: &inputs.release.tag_name,
            groups: &groups,
        },
    )
    .map_err(|error| invalid(&format!("release_config:{error}")))?;
    let approved_json = canonical_json_str(inputs.reconciliation)?;
    Ok(
        json!({"approved_json": approved_json, "release_config": release_config,
        "manifest": inputs.release.manifest_path, "actual_host": actual_host}),
    )
}

fn main_sources() -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut owned = crate::release_emit::release_support_sources::support_sources(VERSION)?;
    owned.extend(velnor_actions_rust::release_support_sources::support_sources(VERSION)?);
    let mut sources = BTreeMap::new();
    for name in MAIN_ORDER {
        if let Some((_, raw)) = LOCAL_MAIN.iter().find(|(local, _)| local == name) {
            let record = CompiledSupportSource::compiled(&format!("{PREFIX}{name}"), raw, VERSION)?;
            insert(&mut sources, name, record.source().to_owned())?;
        } else if *name != "compiled_source_artifact_input.py" {
            let record = owned
                .iter()
                .find(|source| source.path() == format!("{PREFIX}{name}"))
                .ok_or_else(|| invalid("owner_module_missing"))?;
            insert(&mut sources, name, record.source().to_owned())?;
        }
    }
    Ok(sources)
}

fn cold_sources() -> Result<Vec<(&'static str, &'static str)>, OrchestratorError> {
    let inventory = fixed_sources(InventorySourceProgram::OriginalFilesystem);
    if inventory.len() != 8 {
        return Err(invalid("original_inventory_order"));
    }
    let mut sources = vec![LOCAL_COLD[0]];
    sources.extend(inventory);
    sources.extend_from_slice(&LOCAL_COLD[1..]);
    let mut seen = std::collections::BTreeSet::new();
    if sources.len() != 17
        || sources
            .iter()
            .any(|(name, source)| source.is_empty() || !seen.insert(*name))
    {
        return Err(invalid("cold_closure_order"));
    }
    Ok(sources)
}

fn insert(
    sources: &mut BTreeMap<String, String>,
    name: &str,
    source: String,
) -> Result<(), OrchestratorError> {
    if sources.insert(name.to_owned(), source).is_some() {
        return Err(invalid("duplicate_module"));
    }
    Ok(())
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("source_intent_prepared_sources:{problem}"),
    }
}

const LAUNCHER: &str = r#"import builtins, sys, types
from types import MappingProxyType

_cold_order = tuple(_CAPSULE['cold_order'])
_cold_sources = MappingProxyType(dict(_CAPSULE['cold_sources']))
_main_order = tuple(_CAPSULE['main_order'])
_main_sources = MappingProxyType(dict(_CAPSULE['main_sources']))
_configuration = MappingProxyType(dict(_CAPSULE['configuration']))
del _CAPSULE
if (len(_cold_order) != 17 or len(set(_cold_order)) != 17
        or set(_cold_order) != set(_cold_sources)
        or len(_main_order) != 20 or len(set(_main_order)) != 20
        or set(_main_order) != set(_main_sources)
        or any(type(name) is not str or type(source) is not str or not source
               for name, source in (*_cold_sources.items(), *_main_sources.items()))):
    raise SystemExit('prepared_compiled_closure')
_stdlib = frozenset({'ctypes', 'dataclasses', 'errno', 'fcntl', 'hashlib', 'json',
                    'os', 'platform', 'selectors', 'signal', 'stat', 'struct',
                    'subprocess', 'sys', 'tempfile', 'time', 'types'})
_original_import = builtins.__import__
_cold_modules = {}
_cold_codes = {}
for _name in _cold_order:
    if _name in sys.modules:
        raise SystemExit('prepared_compiled_module_collision')
    _cold_codes[_name] = compile(_cold_sources[_name], '<compiled-source:' + _name + '>', 'exec')
    _module = types.ModuleType(_name)
    _module.__file__ = '<compiled-source:' + _name + '>'
    _cold_modules[_name] = _module

def _sealed_import(name, globals=None, locals=None, fromlist=(), level=0):
    if level or type(name) is not str:
        raise ImportError('prepared_compiled_import')
    if name in _cold_modules:
        if sys.modules.get(name) is not _cold_modules[name]:
            raise ImportError('prepared_compiled_module_rebound')
    elif name not in _stdlib:
        raise ImportError('prepared_compiled_import:' + name)
    return _original_import(name, globals, locals, fromlist, level)

_sealed_builtins = dict(vars(builtins))
_sealed_builtins['__import__'] = _sealed_import
_sealed_builtins = MappingProxyType(_sealed_builtins)
for _name, _module in _cold_modules.items():
    _module.__dict__['__builtins__'] = _sealed_builtins
    sys.modules[_name] = _module
for _name in _cold_order:
    exec(_cold_codes[_name], _cold_modules[_name].__dict__)
_namespace = {'__name__': 'velnor_prepared_compiled'}
_sdk = _cold_modules['source_intent_cold_sdk']
for _export in ('ColdSourceIntentSdk', 'ColdSourceIntentInstalledTool', 'load_source_intent_sdk'):
    _namespace[_export] = getattr(_sdk, _export)
for _name in _main_order:
    exec(compile(_main_sources[_name], '<compiled-main:' + _name + '>', 'exec'), _namespace)
_namespace['_COMPILED_PREPARED_CONFIGURATION'] = dict(_configuration)
_namespace['prepare_source_intent']()
"#;
