//! Opaque catalog-owned native preparation for a private receipt composition.
//!
//! Warm bytes are not a separately admitted source helper. A receipt owner must
//! consume its verified grant immediately before executing this fixed recipe.

use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation, ToolCacheDomain};

use crate::{
    MiseError,
    catalog::{ToolCatalog, native_health, qualification::DistributionHost, tool_prepare},
};

/// Captured native owner inputs and the exact source variants they reconstruct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeReceiptPreparation {
    catalog: ToolCatalog,
    domain: ToolCacheDomain,
    host: DistributionHost,
    selectors: Vec<String>,
    version: String,
    cold: CompiledSourceHelper,
    clear: String,
    warm: String,
}

impl NativeReceiptPreparation {
    /// Reconstruct the canonical native owner; caller source never grants authority.
    /// # Errors
    /// Rejects unqualified catalogs, delegated Rust/Gradle recipes and bad selectors.
    pub fn for_tools(
        catalog: &ToolCatalog,
        domain: ToolCacheDomain,
        host: DistributionHost,
        selectors: &[String],
        generator_version: &str,
    ) -> Result<Self, MiseError> {
        if domain == ToolCacheDomain::GradleBootstrap {
            return Err(unsupported());
        }
        let cold =
            tool_prepare::helper_for_tools(catalog, domain, host, selectors, generator_version)?;
        if cold.invocation().descriptor().operation() != SourceBoundOperation::MiseToolPrepare {
            return Err(unsupported());
        }
        Ok(Self {
            catalog: catalog.clone(),
            domain,
            host,
            selectors: selectors.to_vec(),
            version: generator_version.to_owned(),
            cold,
            clear: native_health::root_clear_script(domain),
            warm: tool_prepare::authenticated_preparation_source(generator_version)?,
        })
    }

    /// Rebuild all captured inputs and exact variants through the canonical owner.
    /// # Errors
    /// Rejects any changed recipe, environment, source binding or qualification.
    pub fn verify_fresh(&self) -> Result<(), MiseError> {
        let expected = Self::for_tools(
            &self.catalog,
            self.domain,
            self.host,
            &self.selectors,
            &self.version,
        )?;
        if self != &expected {
            return Err(MiseError::Contract {
                problem: "native_receipt_preparation_changed".to_owned(),
            });
        }
        self.cold
            .validate_binding()
            .map_err(|error| tool_prepare::contract(&error))
    }

    /// Exact domain supplied to the bootstrap and selected-root owner factories.
    #[must_use]
    pub const fn domain(&self) -> ToolCacheDomain {
        self.domain
    }

    /// Compiled distribution host, never inferred from a caller environment.
    #[must_use]
    pub const fn host(&self) -> DistributionHost {
        self.host
    }

    /// Version used by every owner-regenerated source and bootstrap helper.
    #[must_use]
    pub fn generator_version(&self) -> &str {
        &self.version
    }

    /// Standalone cold helper retains its existing admission and verification.
    #[must_use]
    pub const fn cold_helper(&self) -> &CompiledSourceHelper {
        &self.cold
    }

    /// Clear selected root leaves before bootstrap or receipt materialization.
    #[must_use]
    pub fn clear_source(&self) -> &str {
        &self.clear
    }

    /// Verification-only owner source for a private authenticated composition.
    #[must_use]
    pub fn warm_source(&self) -> &str {
        &self.warm
    }
}

fn unsupported() -> MiseError {
    MiseError::Contract {
        problem: "native_receipt_preparation_owner_unavailable".to_owned(),
    }
}
