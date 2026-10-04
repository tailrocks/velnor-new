//! Finite receipt preparation assembled exclusively from reconstructed owners.
use super::{CacheReceiptSources, OrchestratorError, body, config, invalid};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation, Step,
    ToolCacheDescriptor, ToolCacheDomain,
};
use velnor_actions_mise::catalog::native_receipt_preparation::NativeReceiptPreparation;
use velnor_actions_mise::catalog::rust_prepare::RustReceiptPreparation;

/// Exact compiler-issued restore metadata; cache outputs carry no execution grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptPreparationRestoreBinding {
    restore: Step,
    descriptor: ToolCacheDescriptor,
    recipe_sha256: String,
}

impl ReceiptPreparationRestoreBinding {
    /// Exact canonical transport step, including key, paths and environment.
    #[must_use]
    pub fn restore_step(&self) -> &Step {
        &self.restore
    }

    /// Payload domain selected by the independently admitted pure producer.
    #[must_use]
    pub fn domain(&self) -> ToolCacheDomain {
        self.descriptor.domain
    }

    /// Exact independently admitted payload, host and installation identities.
    #[must_use]
    pub fn descriptor(&self) -> &ToolCacheDescriptor {
        &self.descriptor
    }
}

/// Source-issued composite; serialization cannot create its owner authority.
#[derive(Debug, Clone)]
pub struct ReceiptOwnedPreparation {
    helper: CompiledSourceHelper,
    binding: ReceiptPreparationRestoreBinding,
    sources: CacheReceiptSources,
    owner: CapturedPreparation,
}

#[derive(Debug, Clone)]
enum CapturedPreparation {
    Rust(Box<RustReceiptPreparation>),
    Native(Box<NativeReceiptPreparation>),
}

impl ReceiptOwnedPreparation {
    /// Exact complete compiled execution source and owner environment.
    #[must_use]
    pub fn helper(&self) -> &CompiledSourceHelper {
        &self.helper
    }

    /// Exact transport binding reconstructed when this record was minted.
    #[must_use]
    pub fn restore_binding(&self) -> &ReceiptPreparationRestoreBinding {
        &self.binding
    }

    /// Recreate complete authority from privately captured source-owned inputs.
    /// # Errors
    /// Rejects stale sources, changed qualification and any record substitution.
    pub fn verify_fresh(&self) -> Result<(), OrchestratorError> {
        let expected = match &self.owner {
            CapturedPreparation::Rust(owner) => {
                rust_receipt_owned_preparation(&self.sources, owner, &self.binding)?
            }
            CapturedPreparation::Native(owner) => {
                native_receipt_owned_preparation(&self.sources, owner, &self.binding)?
            }
        };
        if self.helper != expected.helper || self.binding != expected.binding {
            return Err(invalid("preparation_complete_owner_record_changed"));
        }
        Ok(())
    }
}

/// Mint restore metadata solely from the admitted pure executable producer.
/// # Errors
/// Rejects source archive roles and altered complete transport steps.
pub fn receipt_preparation_restore_binding(
    sources: &CacheReceiptSources,
    actual: &Step,
) -> Result<ReceiptPreparationRestoreBinding, OrchestratorError> {
    let expected = canonical_binding(sources)?;
    if expected.restore != *actual {
        return Err(invalid("preparation_restore_record_changed"));
    }
    Ok(expected)
}

/// Generate the exact restore transport before admitting the whole binding.
/// # Errors
/// Rejects non-tool producer receipts and malformed owned descriptors.
pub fn receipt_preparation_restore_step(
    sources: &CacheReceiptSources,
) -> Result<Step, OrchestratorError> {
    Ok(canonical_binding(sources)?.restore)
}

fn canonical_binding(
    sources: &CacheReceiptSources,
) -> Result<ReceiptPreparationRestoreBinding, OrchestratorError> {
    let _metadata = sources
        .recipe
        .original()
        .tool_producer
        .as_ref()
        .ok_or_else(|| invalid("preparation_requires_tool_producer"))?;
    // The owned SDK quarantine adapter has no qualified immutable action A.
    // Publication also lacks the exact immutable service cache key. Neither an
    // official direct restore nor writable action outputs can mint this binding.
    Err(invalid("preparation_restore_adapter_unqualified"))
}

/// Compose complete Rust owner sources after fresh captured-input reconstruction.
/// # Errors
/// Rejects foreign receipt roles, altered owner records or mismatched bindings.
pub fn rust_receipt_owned_preparation(
    sources: &CacheReceiptSources,
    owner: &RustReceiptPreparation,
    binding: &ReceiptPreparationRestoreBinding,
) -> Result<ReceiptOwnedPreparation, OrchestratorError> {
    owner.verify_fresh()?;
    require_fresh_sources(sources)?;
    if binding != &canonical_binding(sources)?
        || binding.domain() != ToolCacheDomain::Full
        || binding.descriptor.target != owner.host().abi()
        || owner.generator_version() != sources.recipe.generator_version()
    {
        return Err(invalid("preparation_rust_owner_binding_changed"));
    }
    let bootstrap = velnor_actions_mise::catalog::mise_acquisition::helper_for_domain(
        owner.bootstrap_domain(),
        owner.host(),
        owner.generator_version(),
    )?;
    let helper = compile(
        sources,
        binding,
        owner.cold_helper(),
        &bootstrap,
        owner.clear_source(),
        owner.warm_source(),
    )?;
    Ok(ReceiptOwnedPreparation {
        helper,
        binding: binding.clone(),
        sources: sources.clone(),
        owner: CapturedPreparation::Rust(Box::new(owner.clone())),
    })
}

/// Compose a non-Rust owner without granting authority through cache outputs.
/// # Errors
/// Rejects delegated owner roles, changed inputs and foreign payload domains.
pub fn native_receipt_owned_preparation(
    sources: &CacheReceiptSources,
    owner: &NativeReceiptPreparation,
    binding: &ReceiptPreparationRestoreBinding,
) -> Result<ReceiptOwnedPreparation, OrchestratorError> {
    owner.verify_fresh()?;
    require_fresh_sources(sources)?;
    if binding != &canonical_binding(sources)?
        || binding.domain() != owner.domain()
        || binding.descriptor.target != owner.host().abi()
        || owner.generator_version() != sources.recipe.generator_version()
    {
        return Err(invalid("preparation_native_owner_binding_changed"));
    }
    let bootstrap = velnor_actions_mise::catalog::mise_acquisition::helper_for_domain(
        owner.domain(),
        owner.host(),
        owner.generator_version(),
    )?;
    let helper = compile(
        sources,
        binding,
        owner.cold_helper(),
        &bootstrap,
        owner.clear_source(),
        Some(owner.warm_source()),
    )?;
    Ok(ReceiptOwnedPreparation {
        helper,
        binding: binding.clone(),
        sources: sources.clone(),
        owner: CapturedPreparation::Native(Box::new(owner.clone())),
    })
}

fn compile(
    sources: &CacheReceiptSources,
    binding: &ReceiptPreparationRestoreBinding,
    cold: &CompiledSourceHelper,
    bootstrap: &CompiledSourceHelper,
    clear: &str,
    warm: Option<&str>,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let mut selectors = cold.invocation().installed_selectors().to_vec();
    selectors.sort();
    if selectors != binding.descriptor.selectors
        || cold.environment().get("VELNOR_QUALIFIED_TOOL_IDENTITY")
            != Some(&binding.descriptor.qualification_identity)
        || cold.environment().get("MISE_DATA_DIR").map(String::as_str)
            != Some(binding.domain().root())
    {
        return Err(invalid("preparation_owner_footprint_changed"));
    }
    let mut configuration =
        config::consumer_configuration(&sources.recipe, sources.producer_descriptor())?;
    let mut environment = cold.environment().clone();
    let bindings = stage_bindings(bootstrap, &mut environment);
    let owner_bindings: Vec<_> = cold
        .environment()
        .keys()
        .map(|name| (name.clone(), name.clone()))
        .collect();
    configuration["preparation"] = serde_json::json!({
        "restore": binding.restore,
        "descriptor": binding.descriptor,
        "recipe_sha256": binding.recipe_sha256,
        "stages": [
            {"source": clear, "args": [], "bindings": owner_bindings},
            {"source": bootstrap.source(), "args": bootstrap.invocation().args(),
             "bindings": bindings},
            {"source": cold.source(), "args": cold.invocation().args(),
             "bindings": owner_bindings},
            {"source": warm, "args": cold.invocation().args(), "bindings": owner_bindings},
        ],
    });
    let modules = preparation_modules(&configuration)?;
    let initializer = body::initializer(&configuration, &modules)?;
    let script = format!(
        "set -euo pipefail\n/usr/bin/python3 -I -S <<'VELNOR_RECEIPT_PREPARATION'\n{initializer}\n{LOADER}\n{ENTRYPOINT}\nVELNOR_RECEIPT_PREPARATION\n"
    );
    let source =
        velnor_actions_contract::generated_source(sources.recipe.generator_version(), &script)?;
    let operation = SourceBoundOperation::ReceiptOwnedPreparation;
    let descriptor = SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
    )?;
    let invocation = HelperInvocation::compiled(
        descriptor,
        Vec::new(),
        cold.invocation().installed_selectors().to_vec(),
    )?;
    Ok(CompiledSourceHelper::compiled(invocation, source)?.with_environment(environment))
}

fn preparation_modules(
    configuration: &serde_json::Value,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut modules = body::consumer_modules(configuration, &body::producer_modules())?;
    modules.extend([
        (
            "receipt_fresh_gh_download".into(),
            include_str!("receipt_fresh_gh_download.py").into(),
        ),
        (
            "receipt_fresh_gh_archive".into(),
            include_str!("receipt_fresh_gh_archive.py").into(),
        ),
        (
            "receipt_fresh_gh".into(),
            include_str!("receipt_fresh_gh.py").into(),
        ),
        (
            "receipt_owned_preparation".into(),
            include_str!("receipt_owned_preparation.py").into(),
        ),
    ]);
    Ok(modules)
}

fn require_fresh_sources(sources: &CacheReceiptSources) -> Result<(), OrchestratorError> {
    let expected = super::compile_sources(&sources.recipe)?;
    if sources.producer_descriptor != expected.producer_descriptor
        || sources.transport_layout != expected.transport_layout
        || sources.manifest != expected.manifest
        || sources.bundle != expected.bundle
        || sources.verify != expected.verify
        || sources.producer_closure_sha256 != expected.producer_closure_sha256
        || sources.consumer_closure_sha256 != expected.consumer_closure_sha256
    {
        return Err(invalid("preparation_receipt_sources_changed"));
    }
    Ok(())
}

fn stage_bindings(
    bootstrap: &CompiledSourceHelper,
    environment: &mut BTreeMap<String, String>,
) -> Vec<(String, String)> {
    bootstrap
        .environment()
        .iter()
        .map(|(name, value)| {
            let binding = format!("VELNOR_RECEIPT_STAGE_1_{name}");
            environment.insert(binding.clone(), value.clone());
            (name.clone(), binding)
        })
        .collect()
}

const LOADER: &str = r#"
_ORDER = ('cache_receipt_common', 'cache_receipt_policy', 'metadata_container',
          'opaque_inventory_metadata', 'source_archive_inventory_common',
          'source_archive_inventory_fs', 'source_archive_inventory_leaf',
          'source_archive_inventory_walk', 'source_archive_inventory',
          'cache_receipt_manifest', 'cache_receipt_virtual',
          'cache_receipt_api', 'cache_receipt_gh', 'receipt_fresh_gh_download',
          'receipt_fresh_gh_archive', 'receipt_fresh_gh', 'cache_receipt',
          'cache_receipt_materialize_transaction', 'cache_receipt_materialize',
          'receipt_owned_preparation')
if set(_SOURCES) != set(_ORDER):
    raise RuntimeError('receipt_preparation_source_registry')
for _name in _ORDER:
    _module = types.ModuleType(_name)
    _module.__file__ = '<compiled-source:' + _name + '>'
    sys.modules[_name] = _module
for _name in _ORDER:
    exec(compile(_SOURCES[_name], '<compiled-source:' + _name + '>', 'exec'),
         sys.modules[_name].__dict__)
del _SOURCES
"#;

const ENTRYPOINT: &str = r#"
from receipt_owned_preparation import prepare_owned
_STAGES = tuple((stage['source'], tuple(stage['args']),
                 tuple(tuple(pair) for pair in stage['bindings']))
                for stage in _CONFIG['preparation']['stages'])
prepare_owned(_STAGES)
"#;

#[cfg(test)]
#[path = "cache_receipt_owned_preparation_tests.rs"]
mod tests;
