//! Closed source aggregation for pure cache receipt production and verification.
use crate::OrchestratorError;
use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation};
use velnor_actions_workflow_renderer::cache_producer_workflow::CacheProducerRecipe;

#[path = "cache_receipt_source_admission.rs"]
mod admission;
#[path = "cache_receipt_source_body.rs"]
mod body;
#[path = "cache_receipt_source_config.rs"]
mod config;
#[path = "cache_receipt_source_consumer_runtime.rs"]
mod consumer_runtime;
#[path = "cache_receipt_source_native.rs"]
pub(crate) mod native;
#[path = "cache_receipt_owned_preparation.rs"]
pub mod preparation;
#[path = "cache_receipt_source_projection.rs"]
mod projection;
#[path = "cache_receipt_source_role_config.rs"]
mod role_config;
#[path = "cache_receipt_source_runtime.rs"]
mod runtime;
#[path = "cache_receipt_source_rust.rs"]
mod rust;
#[path = "cache_receipt_transport_layout.rs"]
mod transport_layout;
#[path = "cache_receipt_trust_root.rs"]
mod trust_root;

pub use transport_layout::CacheReceiptTransportLayout;

/// Complete immutable source records for one independently admitted pure recipe.
#[derive(Debug, Clone)]
pub struct CacheReceiptSources {
    recipe: CacheProducerRecipe,
    producer_descriptor: serde_json::Value,
    transport_layout: CacheReceiptTransportLayout,
    manifest: CompiledSourceHelper,
    bundle: CompiledSourceHelper,
    verify: CompiledSourceHelper,
    producer_closure_sha256: String,
    consumer_closure_sha256: String,
}

impl CacheReceiptSources {
    /// Exact generation-owned producer policy snapshot, including its final policy hash.
    /// Consumer publication must preserve these values rather than recompute a catalog.
    #[must_use]
    pub fn producer_descriptor(&self) -> &serde_json::Value {
        &self.producer_descriptor
    }
    /// Exact ordered SDK path bytes and numbered quarantine mapping.
    #[must_use]
    pub fn transport_layout(&self) -> &CacheReceiptTransportLayout {
        &self.transport_layout
    }
    /// Actual output inventory and producer predicate; never caller JSON.
    #[must_use]
    pub fn manifest(&self) -> &CompiledSourceHelper {
        &self.manifest
    }
    /// Public attestation bundle transport, without credentials.
    #[must_use]
    pub fn bundle(&self) -> &CompiledSourceHelper {
        &self.bundle
    }
    /// Consumer verifier; cold until actual publication is qualified.
    #[must_use]
    pub fn verify(&self) -> &CompiledSourceHelper {
        &self.verify
    }
    /// Frozen producer modules, loader and entrypoint identity; excludes publication.
    #[must_use]
    pub fn producer_closure_sha256(&self) -> &str {
        &self.producer_closure_sha256
    }
    /// Consumer modules, loader and entrypoint identity; independent of producer identity.
    #[must_use]
    pub fn consumer_closure_sha256(&self) -> &str {
        &self.consumer_closure_sha256
    }
    pub(crate) fn admits(&self, recipe: &CacheProducerRecipe) -> bool {
        self.recipe.original() == recipe.original()
            && self.recipe.source_helpers() == recipe.source_helpers()
            && self.recipe.generator_version() == recipe.generator_version()
    }
}

/// Actual hosted/source publication authority. No shape-only public constructor.
#[derive(Debug, Clone)]
pub struct CacheReceiptPublication {
    _private: (),
}

/// No public or private recipe has completed immutable hosted qualification yet.
#[must_use]
pub fn qualified_cache_receipt_publication() -> Option<CacheReceiptPublication> {
    None
}

/// Create publication drafts from opaque admitted computation, never arbitrary steps.
/// Producer operations run; consumer admission remains cold without publication proof.
/// # Errors
/// Rejects unknown hosts, missing GH source records and unsupported payload roots.
pub fn draft_cache_receipt_sources(
    recipe: &CacheProducerRecipe,
) -> Result<CacheReceiptSources, OrchestratorError> {
    admission::validate(recipe)?;
    compile_sources(recipe)
}

/// Admit native generation only with fresh whole-factory typed input authority.
pub(crate) fn draft_cache_receipt_sources_with_native(
    recipe: &CacheProducerRecipe,
    owner: &native::NativeReceiptRecipe,
) -> Result<CacheReceiptSources, OrchestratorError> {
    admission::validate_with_native(recipe, owner)?;
    compile_sources(recipe)
}

/// MBX signing drafts require the independent complete compiled data owner.
pub(crate) fn draft_cache_receipt_sources_with_mbx(
    recipe: &CacheProducerRecipe,
    owner: &crate::mbx_producer::DraftMbxProducer,
) -> Result<CacheReceiptSources, OrchestratorError> {
    admission::validate_with_mbx(recipe, owner)?;
    compile_sources(recipe)
}

fn compile_sources(recipe: &CacheProducerRecipe) -> Result<CacheReceiptSources, OrchestratorError> {
    let mut configuration = config::configuration(recipe)?;
    let modules = body::producer_modules();
    let producer_closure_sha256 = body::closure_sha256(&modules, false)?;
    configuration["policy_sha256"] =
        serde_json::Value::String(velnor_actions_contract::compiled_source_sha256(
            velnor_actions_contract::canonical_json_str(&serde_json::json!({
                "unsigned_producer_descriptor": configuration,
                "producer_closure_sha256": producer_closure_sha256,
                "attest_uses": "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6",
            }))?
            .as_bytes(),
        ));
    let transport_layout = transport_layout::derive_for_descriptor(recipe, &configuration)?;
    let manifest = body::record(
        recipe,
        SourceBoundOperation::CacheReceiptManifest,
        "manifest",
        &configuration,
        &modules,
    )?;
    let bundle = body::record(
        recipe,
        SourceBoundOperation::CacheReceiptBundle,
        "bundle",
        &configuration,
        &modules,
    )?;
    let consumer_configuration = config::consumer_configuration(recipe, &configuration)?;
    let consumer_modules = body::consumer_modules(&consumer_configuration, &modules)?;
    let consumer_closure_sha256 =
        body::consumer_closure_sha256(&consumer_modules, &consumer_configuration)?;
    let verify = body::record(
        recipe,
        SourceBoundOperation::CacheReceiptVerify,
        "verify",
        &consumer_configuration,
        &consumer_modules,
    )?;
    Ok(CacheReceiptSources {
        recipe: recipe.clone(),
        producer_descriptor: configuration,
        transport_layout,
        manifest,
        bundle,
        verify,
        producer_closure_sha256,
        consumer_closure_sha256,
    })
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("cache_receipt_source_{problem}"),
    }
}

#[cfg(test)]
#[path = "cache_receipt_source_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cache_receipt_source_identity_tests.rs"]
mod identity_tests;

#[cfg(test)]
#[path = "cache_receipt_source_mbx_tests.rs"]
mod mbx_tests;
