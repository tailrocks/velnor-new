//! Closed isolated producer authority and terminal evidence bindings.
use super::{source_helper::SourceBoundOperation, step::StepId};
use crate::ContractError;
use serde::{Deserialize, Serialize};

/// Native source formats admitted for isolated production.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceProducerRole {
    /// Public Cargo registry source archives.
    Cargo,
    /// Public npm content-addressed source objects.
    Npm,
    /// Public frozen Bun source objects.
    Bun,
    /// Public dependency-only Gradle module artifacts.
    Gradle,
    /// Public immutable `OpenTofu` providers.
    Tofu,
}

impl SourceProducerRole {
    /// Exact compiled operations allowed within this role.
    #[must_use]
    pub const fn permits(self, operation: SourceBoundOperation) -> bool {
        match operation {
            SourceBoundOperation::CacheSnapshot | SourceBoundOperation::SourceProducerReport => {
                true
            }
            SourceBoundOperation::NpmPublicSourceProducer => matches!(self, Self::Npm),
            SourceBoundOperation::BunSourceProducer => matches!(self, Self::Bun),
            SourceBoundOperation::GradleSourceProducer => matches!(self, Self::Gradle),
            SourceBoundOperation::RustSourceProducer => matches!(self, Self::Cargo),
            SourceBoundOperation::TofuProviderExport | SourceBoundOperation::TofuRootOwnership => {
                matches!(self, Self::Tofu)
            }
            _ => false,
        }
    }
}

/// Literal source identity and mandatory proof/transport bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceProducer {
    /// Closed native producer role.
    pub role: SourceProducerRole,
    /// Exact selected task authority for this source cohort.
    pub selection: super::tool_producer_selection::ToolProducerSelection,
    /// Optional read-only bootstrap payload qualified by its compiled owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_cache: Option<super::tool_producer::ToolCacheDescriptor>,
    /// Full immutable source key; runtime expressions are forbidden.
    pub source_identity: String,
    /// Compiled source verification step.
    pub verification_step: StepId,
    /// Candidate restore transport used to prove already published availability.
    pub restore_step: StepId,
    /// Qualified cache save transport step.
    pub save_step: StepId,
    /// Exact-key lookup after attempted publication.
    pub publication_step: StepId,
    /// Mandatory terminal evidence owner.
    pub report_step: StepId,
}

impl SourceProducer {
    /// Exact task and protected default-head allocation.
    #[must_use]
    pub fn condition(&self) -> String {
        self.selection.condition(match self.role {
            SourceProducerRole::Cargo => super::tool_producer::ToolCacheDomain::Full,
            SourceProducerRole::Npm => super::tool_producer::ToolCacheDomain::NpmBootstrap,
            SourceProducerRole::Bun => super::tool_producer::ToolCacheDomain::BunBootstrap,
            SourceProducerRole::Tofu => super::tool_producer::ToolCacheDomain::TofuBootstrap,
            SourceProducerRole::Gradle => super::tool_producer::ToolCacheDomain::GradleBootstrap,
        })
    }

    /// Canonical native cache publication key.
    #[must_use]
    pub fn save_key(&self) -> String {
        let digest = match self.role {
            SourceProducerRole::Cargo => "VELNOR_SOURCES_SNAPSHOT_DIGEST",
            SourceProducerRole::Npm => "VELNOR_NPM_DOWNLOADS_SNAPSHOT_DIGEST",
            SourceProducerRole::Bun => "VELNOR_BUN_DOWNLOADS_SNAPSHOT_DIGEST",
            _ => return self.source_identity.clone(),
        };
        format!(
            "{}-snapshot-${{{{env.{digest}}}}}-${{{{github.run_id}}}}-${{{{github.run_attempt}}}}",
            self.source_identity
        )
    }

    /// Publication evidence lookup runs even after an optional transport failure.
    #[must_use]
    pub fn publication_condition(&self) -> String {
        format!(
            "always() && {} && steps.{}.outputs.verified == 'true'",
            super::cache_trust::CACHE_TRUSTED_PUSH_EXPR,
            self.verification_step.as_str()
        )
    }

    /// Exact successful trusted source publication predicate.
    #[must_use]
    pub fn save_condition(&self) -> String {
        let verified = format!(
            "{} && steps.{}.outputs.verified == 'true'",
            super::cache_trust::CACHE_SAVE_CONDITION,
            self.verification_step.as_str()
        );
        match self.role {
            SourceProducerRole::Cargo => {
                format!("{verified} && env.VELNOR_SOURCES_SNAPSHOT_CHANGED == 'true'")
            }
            SourceProducerRole::Npm => {
                format!("{verified} && env.VELNOR_NPM_DOWNLOADS_SNAPSHOT_CHANGED == 'true'")
            }
            SourceProducerRole::Bun => {
                format!("{verified} && env.VELNOR_BUN_DOWNLOADS_SNAPSHOT_CHANGED == 'true'")
            }
            SourceProducerRole::Tofu | SourceProducerRole::Gradle => verified,
        }
    }

    /// Validate literal source authority.
    /// # Errors
    /// Rejects an empty key, expressions, or unsafe key bytes.
    pub fn validate(&self) -> Result<(), ContractError> {
        self.selection.validate()?;
        if self.selection.cargo_fallback && self.role != SourceProducerRole::Cargo {
            return Err(ContractError::identity(
                "source_producer",
                "cargo_fallback_forbidden",
            ));
        }
        if let Some(tool) = &self.tool_cache {
            tool.validate()?;
            let domain = match self.role {
                SourceProducerRole::Cargo => super::tool_producer::ToolCacheDomain::Full,
                SourceProducerRole::Npm => super::tool_producer::ToolCacheDomain::NpmBootstrap,
                SourceProducerRole::Bun => super::tool_producer::ToolCacheDomain::BunBootstrap,
                SourceProducerRole::Tofu => super::tool_producer::ToolCacheDomain::TofuBootstrap,
                SourceProducerRole::Gradle => {
                    super::tool_producer::ToolCacheDomain::GradleBootstrap
                }
            };
            if tool.domain != domain {
                return Err(ContractError::identity(
                    "source_producer",
                    "foreign_bootstrap_domain",
                ));
            }
        }
        self.verification_step.validate()?;
        self.save_step.validate()?;
        self.restore_step.validate()?;
        self.publication_step.validate()?;
        self.report_step.validate()?;
        if self.source_identity.is_empty()
            || !self
                .source_identity
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            return Err(ContractError::identity(
                "source_producer",
                "invalid_source_identity",
            ));
        }
        Ok(())
    }
}

/// Shared closed terminal reasons; availability and CI conclusions are independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProducerTerminalError {
    /// Complete verified payload and available transport.
    None,
    /// Candidate needs credentials or public authority cannot be proven.
    PrivateOrAuthRequired,
    /// Public authority could not be established from anonymous evidence.
    PublicAuthorityUnavailable,
    /// Registry is outside the compiled public-source policy.
    UnsupportedRegistry,
    /// Verified bytes have no proven publication under the exact source identity.
    CacheNotPublished,
    /// Bootstrap or source preparation did not complete.
    PreparationFailed,
    /// Native ingestion or source integrity failed.
    SourceVerificationFailed,
    /// A cache operation failed; exact current-key availability remains independent.
    CacheTransportFailed,
    /// Exact-key publication lookup did not establish remote availability.
    CacheTransportUnavailable,
}
