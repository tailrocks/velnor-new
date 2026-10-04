//! Neutral owner-qualified Mise acquisition records; no action or catalog policy.
use std::collections::BTreeMap;
use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation, Step, ToolCacheDomain};

use crate::RenderError;

/// Contract-fixed display name of the acquisition step.
pub const SETUP_MISE_NAME: &str = "Acquire qualified Mise";

/// Exact installed binary identity bound to its compiled acquisition authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseBootstrap {
    /// Source-owned executable target for this runner label.
    pub target: String,
    /// SHA-256 of the extracted executable, never the release archive.
    pub binary_sha256: String,
    /// Source owner record, with an exact closed-domain environment.
    pub helper: CompiledSourceHelper,
}

/// Acquisition authority indexed by closed domain and actual final job label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseSetup {
    /// Exact owned Mise release identity.
    pub version: String,
    /// Every entry comes from the compiled Mise owner; absent entries fail closed.
    pub bootstraps: BTreeMap<(ToolCacheDomain, String), MiseBootstrap>,
}

impl MiseSetup {
    /// Validate neutral bindings before constructing workflow steps.
    /// # Errors
    /// Rejects absent authority or mismatched binary, version, domain or operation.
    pub fn validate(&self) -> Result<(), RenderError> {
        if !is_catalog_version(&self.version) || self.bootstraps.is_empty() {
            return Err(invalid("mise_bootstrap_authority_missing"));
        }
        for ((domain, label), bootstrap) in &self.bootstraps {
            let invocation = bootstrap.helper.invocation();
            invocation.validate().map_err(RenderError::Contract)?;
            let env = bootstrap.helper.environment();
            if label.trim().is_empty()
                || label.contains("${{")
                || label.chars().any(char::is_control)
                || invocation.descriptor().operation() != SourceBoundOperation::MiseBootstrap
                || velnor_actions_contract::tool_target_for_runner_label(label)
                    != Some(bootstrap.target.as_str())
                || env.get("VELNOR_MISE_TARGET") != Some(&bootstrap.target)
                || !invocation.installed_selectors().is_empty()
                || !velnor_actions_contract::ids::is_lower_hex_len(&bootstrap.binary_sha256, 64)
                || env.get("VELNOR_MISE_SHA256") != Some(&bootstrap.binary_sha256)
                || env.get("VELNOR_MISE_VERSION") != Some(&self.version)
                || env.get("MISE_DATA_DIR").map(String::as_str) != Some(domain.root())
                || env
                    .get("VELNOR_QUALIFIED_TOOL_IDENTITY")
                    .is_none_or(String::is_empty)
            {
                return Err(invalid("mise_bootstrap_binding_changed"));
            }
        }
        Ok(())
    }

    /// Select authority using the domain and enclosing job's actual label.
    /// # Errors
    /// Rejects missing or malformed source qualification.
    pub fn bootstrap(
        &self,
        domain: ToolCacheDomain,
        runs_on: &str,
    ) -> Result<&MiseBootstrap, RenderError> {
        self.validate()?;
        self.bootstraps
            .get(&(domain, runs_on.to_owned()))
            .ok_or_else(|| invalid("mise_bootstrap_host_domain_unqualified"))
    }
}

/// Build exact acquisition authority for one final job and tool domain.
/// # Errors
/// Rejects absent qualification or changed source owner environment.
pub fn mise_setup_step(
    setup: &MiseSetup,
    domain: ToolCacheDomain,
    runs_on: &str,
) -> Result<Step, RenderError> {
    let bootstrap = setup.bootstrap(domain, runs_on)?;
    crate::source_helper::source_helper_step(
        SETUP_MISE_NAME,
        &bootstrap.helper,
        bootstrap.helper.environment().clone(),
    )
}

fn invalid(reason: &str) -> RenderError {
    RenderError::BadCommand(reason.to_owned())
}

fn is_catalog_version(value: &str) -> bool {
    !value.is_empty()
        && !value.contains("latest")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && value.contains('.')
        && !value.contains("${{")
}

#[cfg(test)]
#[path = "setup_fixture.rs"]
pub(crate) mod fixture;

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
