//! Fixed two-domain release admission authority, distinct from source production.
//!
//! Both domains need independent cold preparation and runtime admission before
//! launch. No compiler or source snapshot semantic authority enters this type.

use std::collections::BTreeMap;

use super::control_parts::{NativeControlParts, qualified_control_parts};
use super::{DistributionHost, MiseError, QualifiedNativeLaunch, ToolCatalog};

/// Closed release admission context; never deserialized or caller constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedAdmissionContext {
    parts: NativeControlParts,
}

/// Resolve the fixed Python Full and GitHub Planning release admission roles.
/// # Errors
/// Rejects missing owned Mise publication or either host's measured native plan.
pub fn qualified_admission_context(
    catalog: &ToolCatalog,
    host: DistributionHost,
) -> Result<QualifiedAdmissionContext, MiseError> {
    Ok(QualifiedAdmissionContext {
        parts: qualified_control_parts(catalog, host)?,
    })
}

impl QualifiedAdmissionContext {
    /// Full Python interpreter for fixed admission operations.
    #[must_use]
    pub const fn python_full(&self) -> &QualifiedNativeLaunch {
        &self.parts.python
    }
    /// Planning GitHub executable, excluded from Full selectors.
    #[must_use]
    pub const fn gh_planning(&self) -> &QualifiedNativeLaunch {
        &self.parts.github
    }
    /// Complete fixed measured native environment without compiler homes.
    #[must_use]
    pub const fn environment(&self) -> &BTreeMap<String, String> {
        &self.parts.environment
    }
    /// Exact Full Mise footprint: Python only.
    #[must_use]
    pub fn full_selectors(&self) -> Vec<String> {
        vec![self.parts.python.selector().to_owned()]
    }
    /// Canonical combined qualification for composing both preparation receipts.
    #[must_use]
    pub fn qualification_digest(&self) -> &str {
        &self.parts.qualification_digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_never_grants_unqualified_host_or_manager_authority() {
        let catalog = ToolCatalog::pinned();
        for host in [
            DistributionHost::LinuxAmd64,
            DistributionHost::LinuxArm64,
            DistributionHost::MacosArm64,
        ] {
            assert!(qualified_admission_context(&catalog, host).is_err());
        }
    }
}
