//! Unregistered original-archive Verify source composer; no runtime qualification.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use velnor_actions_contract::{CompiledSupportSource, canonical_json_str};
use velnor_actions_mise::source_archive_inventory::{InventorySourceProgram, fixed_sources};

use super::prepared_input::CompiledPreparedArtifactInput;
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
    "compiled_source_artifact_input.py",
    "release_source_intent_contract.py",
    "release_source_intent_guard.py",
    "release_prepared_artifact_input.py",
    "compiled_prepared_artifact_input.py",
    "release_source_intent_verification_context.py",
    "release_source_intent_verify.py",
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
        "release_prepared_artifact_input.py",
        include_str!("release_prepared_artifact_input.py"),
    ),
    (
        "release_source_intent_verification_context.py",
        include_str!("release_source_intent_verification_context.py"),
    ),
];
const NATIVE_SDK: (&str, &str) = (
    "source_intent_native_verifier",
    include_str!("source_intent_native_verifier.py"),
);

/// Source bytes and the authenticated source transport stay in one private record.
/// No Prepared producer capability or Native SDK authority is issued here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompiledVerifiedSource {
    body: String,
    environment: BTreeMap<String, String>,
}

impl CompiledVerifiedSource {
    pub(super) fn compile(inputs: &JobInputs<'_>) -> Result<Self, OrchestratorError> {
        let configuration = configuration(inputs)?;
        let input = CompiledSourceArtifactInput::compile(inputs)?;
        let prepared = CompiledPreparedArtifactInput::compile(inputs)?;
        let environment = merge_environment(input.environment(), prepared.environment())?;
        Ok(Self {
            body: compose(&input, &prepared, configuration)?,
            environment,
        })
    }

    pub(super) fn body(&self) -> &str {
        &self.body
    }

    pub(super) fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}

fn configuration(inputs: &JobInputs<'_>) -> Result<Value, OrchestratorError> {
    let actual_host = velnor_actions_contract::tool_target_for_runner_label(inputs.label)
        .filter(|host| *host == "x86_64-unknown-linux-gnu")
        .ok_or_else(|| invalid("actual_host"))?;
    let approved_json = canonical_json_str(inputs.reconciliation)?;
    Ok(json!({"approved_json": approved_json,
        "manifest": inputs.release.manifest_path, "actual_host": actual_host}))
}

fn compose(
    input: &CompiledSourceArtifactInput,
    prepared: &CompiledPreparedArtifactInput,
    configuration: Value,
) -> Result<String, OrchestratorError> {
    let mut main = main_sources()?;
    insert(
        &mut main,
        "compiled_source_artifact_input.py",
        input.runtime_source()?,
    )?;
    insert(
        &mut main,
        "compiled_prepared_artifact_input.py",
        prepared.runtime_source()?,
    )?;
    let native = native_sources()?;
    let capsule = canonical_json_str(&json!({
        "configuration": configuration,
        "main_order": MAIN_ORDER,
        "main_sources": main,
        "native_order": native.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        "native_sources": native.into_iter().collect::<BTreeMap<_, _>>(),
    }))?
    .replace('$', "\\u0024");
    if capsule.contains("\"\"\"") {
        return Err(invalid("capsule_literal_delimiter"));
    }
    Ok(format!(
        "set -euo pipefail\npython3 -I -S - \"$@\" <<'VELNOR_VERIFY_COMPILED_BODY'\n\
         import json\n_CAPSULE = json.loads(r\"\"\"{capsule}\"\"\")\n\
         {LAUNCHER}\nVELNOR_VERIFY_COMPILED_BODY\n"
    ))
}

fn main_sources() -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut owned = crate::release_emit::release_support_sources::support_sources(VERSION)?;
    owned.extend(velnor_actions_rust::release_support_sources::support_sources(VERSION)?);
    let mut sources = BTreeMap::new();
    for name in MAIN_ORDER {
        if let Some((_, raw)) = LOCAL_MAIN.iter().find(|(local, _)| local == name) {
            let record = CompiledSupportSource::compiled(&format!("{PREFIX}{name}"), raw, VERSION)?;
            insert(&mut sources, name, record.source().to_owned())?;
        } else if !matches!(
            *name,
            "compiled_source_artifact_input.py" | "compiled_prepared_artifact_input.py"
        ) {
            let record = owned
                .iter()
                .find(|source| source.path() == format!("{PREFIX}{name}"))
                .ok_or_else(|| invalid("owner_module_missing"))?;
            insert(&mut sources, name, record.source().to_owned())?;
        }
    }
    Ok(sources)
}

fn native_sources() -> Result<Vec<(&'static str, &'static str)>, OrchestratorError> {
    let mut sources = fixed_sources(InventorySourceProgram::OriginalFilesystem);
    if sources.len() != 8 {
        return Err(invalid("original_inventory_order"));
    }
    sources.push(NATIVE_SDK);
    let mut seen = std::collections::BTreeSet::new();
    if sources.len() != 9
        || sources
            .iter()
            .any(|(name, source)| source.is_empty() || !seen.insert(*name))
    {
        return Err(invalid("native_closure_order"));
    }
    Ok(sources)
}

fn merge_environment(
    source: &BTreeMap<String, String>,
    prepared: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut environment = source.clone();
    for (name, value) in prepared {
        if environment
            .get(name)
            .is_some_and(|existing| existing != value)
        {
            return Err(invalid("transport_environment_conflict"));
        }
        environment.insert(name.clone(), value.clone());
    }
    Ok(environment)
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
        problem: format!("source_intent_verify_sources:{problem}"),
    }
}

const LAUNCHER: &str = r#"import builtins, sys, types
from types import MappingProxyType

_native_order = tuple(_CAPSULE['native_order'])
_native_sources = MappingProxyType(dict(_CAPSULE['native_sources']))
_main_order = tuple(_CAPSULE['main_order'])
_main_sources = MappingProxyType(dict(_CAPSULE['main_sources']))
_configuration = MappingProxyType(dict(_CAPSULE['configuration']))
del _CAPSULE
if (len(_native_order) != 9 or len(set(_native_order)) != 9
        or set(_native_order) != set(_native_sources)
        or len(_main_order) != 18 or len(set(_main_order)) != 18
        or set(_main_order) != set(_main_sources)
        or any(type(name) is not str or type(source) is not str or not source
               for name, source in (*_native_sources.items(), *_main_sources.items()))):
    raise SystemExit('verify_compiled_closure')
_stdlib = frozenset({'ctypes', 'dataclasses', 'errno', 'fcntl', 'hashlib', 'json',
                    'os', 'platform', 'selectors', 'signal', 'stat', 'struct',
                    'subprocess', 'sys', 'tempfile', 'time', 'types'})
_original_import = builtins.__import__
_native_modules = {}
_native_codes = {}
for _name in _native_order:
    if _name in sys.modules:
        raise SystemExit('verify_compiled_module_collision')
    _native_codes[_name] = compile(_native_sources[_name], '<compiled-source:' + _name + '>', 'exec')
    _module = types.ModuleType(_name)
    _module.__file__ = '<compiled-source:' + _name + '>'
    _native_modules[_name] = _module

def _sealed_import(name, globals=None, locals=None, fromlist=(), level=0):
    if level or type(name) is not str:
        raise ImportError('verify_compiled_import')
    if name in _native_modules:
        if sys.modules.get(name) is not _native_modules[name]:
            raise ImportError('verify_compiled_module_rebound')
    elif name not in _stdlib:
        raise ImportError('verify_compiled_import:' + name)
    return _original_import(name, globals, locals, fromlist, level)

_sealed_builtins = dict(vars(builtins))
_sealed_builtins['__import__'] = _sealed_import
_sealed_builtins = MappingProxyType(_sealed_builtins)
for _name, _module in _native_modules.items():
    _module.__dict__['__builtins__'] = _sealed_builtins
    sys.modules[_name] = _module
for _name in _native_order:
    exec(_native_codes[_name], _native_modules[_name].__dict__)
_namespace = {'__name__': 'velnor_verify_compiled'}
_sdk = _native_modules['source_intent_native_verifier']
for _export in ('NativeVerifierSdk', 'load_native_verifier_sdk'):
    _namespace[_export] = getattr(_sdk, _export)
for _name in _main_order:
    exec(compile(_main_sources[_name], '<compiled-main:' + _name + '>', 'exec'), _namespace)
_session_type = _namespace['_OriginalArchiveVerificationSession']
_validator = _namespace['validate_original_archive_verification_session']
if (not isinstance(_session_type, type) or not callable(_validator)
        or _validator.__globals__ is not _namespace
        or _session_type.require_current.__globals__ is not _namespace):
    raise SystemExit('verify_compiled_session_binding')
_sdk.__dict__['_OriginalArchiveVerificationSession'] = _session_type
_sdk.__dict__['validate_original_archive_verification_session'] = _validator
_namespace['_COMPILED_VERIFICATION_CONFIGURATION'] = dict(_configuration)
_namespace['verify_source_intent']()
"#;
