//! Sealed continuation sources for the receipt owner's same-process admission.

use std::collections::BTreeMap;

use velnor_actions_contract::{CompiledSourceHelper, HelperInvocation, ToolCacheDomain};

use super::{
    DistributionHost, DistributionRequirement, DistributionTool, MiseError, QualifiedDistribution,
    RustPrepareDomain, ToolCatalog, distribution_host, helper_for_install, role_for_catalog,
};

use super::body;

/// Owner-generated sources; warm bytes cannot be an independent helper record.
///
/// This type carries no runtime grant. Only the receipt composite may launch the
/// warm continuation after authenticated materialization in its own process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustReceiptPreparation {
    cold: CompiledSourceHelper,
    warm: Option<String>,
    manager: String,
    clear: String,
    host: DistributionHost,
    request: ReceiptRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReceiptRequest {
    catalog: ToolCatalog,
    domain: RustPrepareDomain,
    install: Vec<String>,
    version: String,
}

impl RustReceiptPreparation {
    /// Exact independently admitted cold invocation metadata.
    #[must_use]
    pub fn invocation(&self) -> &HelperInvocation {
        self.cold.invocation()
    }

    /// Exact owner environment for both composite continuations.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        self.cold.environment()
    }

    /// Canonical cold installation source, retained for an unauthenticated miss.
    #[must_use]
    pub fn cold_source(&self) -> &str {
        self.cold.source()
    }

    /// Terminal verification only; never admitted by standalone reconstruction.
    #[must_use]
    pub fn warm_source(&self) -> Option<&str> {
        self.warm.as_deref()
    }

    /// Pinned Rustup verification source, with no acquisition or initialization.
    #[must_use]
    pub fn manager_source(&self) -> &str {
        &self.manager
    }

    /// Exact cold helper authority; no warm helper is constructed.
    #[must_use]
    pub const fn cold_helper(&self) -> &CompiledSourceHelper {
        &self.cold
    }

    /// Fixed root clearing source, executed before acquiring the manager.
    #[must_use]
    pub fn clear_source(&self) -> &str {
        &self.clear
    }

    /// Exact compiled manager host.
    #[must_use]
    pub const fn host(&self) -> DistributionHost {
        self.host
    }

    /// Closed bootstrap namespace, independent of the Full payload namespace.
    #[must_use]
    pub const fn bootstrap_domain(&self) -> ToolCacheDomain {
        self.request.domain.bootstrap_domain()
    }

    /// Rust tool payloads always belong to the Full cache domain.
    #[must_use]
    pub const fn payload_domain(&self) -> ToolCacheDomain {
        ToolCacheDomain::Full
    }

    /// Version used to regenerate every exact owner source.
    #[must_use]
    pub fn generator_version(&self) -> &str {
        &self.request.version
    }

    /// Reconstruct all sources and metadata rather than trusting captured bytes.
    /// # Errors
    /// Rejects stale qualification, altered source, or changed owner metadata.
    pub fn verify_fresh(&self) -> Result<(), MiseError> {
        let fresh = receipt_preparation_for_install(
            &self.request.catalog,
            self.request.domain,
            &self.request.install,
            &self.request.version,
        )?;
        if &fresh != self {
            return Err(super::invalid());
        }
        Ok(())
    }
}

/// Construct exact receipt continuations from the same cold preparation owner.
/// # Errors
/// Rejects foreign selectors and missing qualified owner distribution records.
pub fn receipt_preparation_for_install(
    catalog: &ToolCatalog,
    domain: RustPrepareDomain,
    install: &[String],
    version: &str,
) -> Result<RustReceiptPreparation, MiseError> {
    let cold = helper_for_install(catalog, domain, install, version)?;
    let role = role_for_catalog(catalog)?;
    let host = distribution_host(role);
    let mise = QualifiedDistribution::require_for_generator(
        DistributionTool::Mise,
        host,
        DistributionRequirement::RequiresNoMiserc,
    )?;
    // No shared archive projection is qualified. A receipt cannot grant warm
    // execution until the selected-tree health policy and Full restore agree.
    let request = ReceiptRequest {
        catalog: catalog.clone(),
        domain,
        install: install.to_vec(),
        version: version.to_owned(),
    };
    Ok(RustReceiptPreparation {
        cold,
        warm: None,
        manager: body::manager_verification(catalog),
        clear: crate::catalog::rust_cold::clear_script(),
        host: mise.host(),
        request,
    })
}

#[cfg(test)]
fn warm_source(
    role: super::RustCompilerRole,
    version: &str,
    mise: &super::QualifiedDistribution,
    mbx: Option<&super::QualifiedDistribution>,
    manager: &str,
) -> Result<String, MiseError> {
    let catalog = super::catalog_for_role(role)?;
    let health = crate::catalog::rust_health::RustToolchainHealth::for_catalog(
        &catalog,
        catalog.rust_install_options(),
    );
    let wrapper = if catalog.rust_uses_mbx() {
        body::WRAPPER_VERIFY
    } else {
        ""
    };
    let body = format!(
        "set -euo pipefail\nexport PATH=/usr/bin:/bin:/usr/sbin:/sbin\n{}\n{manager}\n{}\n{}\n{wrapper}\nprintf '%s\\n' \"$CARGO_HOME/bin\" >> \"${{GITHUB_PATH:?missing runner path file}}\"\nif test -n \"${{GITHUB_OUTPUT:-}}\"; then printf 'verified=true\\n' >> \"$GITHUB_OUTPUT\"; fi\n",
        body::mise_verification(role, mise),
        body::mbx_verification(mbx, catalog.rust_uses_mbx())?,
        health.terminal_restore_script()?,
    );
    velnor_actions_contract::generated_source(version, &body).map_err(super::contract)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::rust_desktop::RustCompilerRole;

    #[test]
    fn warm_source_verifies_without_repair_or_independent_admission() -> Result<(), MiseError> {
        for role in [
            RustCompilerRole::RootLinux,
            RustCompilerRole::DesktopMac,
            RustCompilerRole::DesktopSourceMac,
            RustCompilerRole::ReleaseMac,
        ] {
            let catalog = super::super::catalog_for_role(role)?;
            // Audit-only source fixture; never creates a production warm record.
            let mise = QualifiedDistribution::qualify_official(
                DistributionTool::Mise,
                distribution_host(role),
            )?;
            let manager = body::manager_verification(&catalog);
            let source = warm_source(role, "0.1.0", &mise, None, &manager)?;
            assert!(source.contains(catalog.rust_host().sha256()));
            for repair in [
                "curl ",
                "reshim --force",
                "install \"$@\"",
                "toolchain uninstall",
                "rust_cold_prepare",
                "set auto-self-update",
                "printf '%s:%s",
            ] {
                assert!(!source.contains(repair), "warm source contains {repair}");
            }
            let mut install = super::super::install_prefix();
            install.push(catalog.tool_spec(catalog.compiler_tool())?);
            assert!(
                receipt_preparation_for_install(
                    &catalog,
                    RustPrepareDomain::Tools,
                    &install,
                    "0.1.0",
                )
                .is_err()
            );
        }
        Ok(())
    }
}
