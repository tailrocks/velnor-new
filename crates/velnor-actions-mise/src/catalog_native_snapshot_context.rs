//! Fixed two-domain source snapshot control; generation authority only.
//!
//! Both domains require their own cold preparation and runtime admission before
//! launch. Full Mise executes Python only; GitHub executes from Planning.

use std::collections::BTreeMap;

use super::control_parts::{NativeControlParts, qualified_control_parts};
use super::{DistributionHost, MiseError, QualifiedNativeLaunch, ToolCatalog};

/// Closed two-domain control context, never deserialized or caller constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedSourceSnapshotContext {
    parts: NativeControlParts,
}

/// Resolve the fixed Python Full and GitHub Planning control roles.
/// # Errors
/// Rejects missing owned Mise publication or either host's native installation plan.
pub fn qualified_source_snapshot_context(
    catalog: &ToolCatalog,
    host: DistributionHost,
) -> Result<QualifiedSourceSnapshotContext, MiseError> {
    Ok(QualifiedSourceSnapshotContext {
        parts: qualified_control_parts(catalog, host)?,
    })
}

impl QualifiedSourceSnapshotContext {
    /// Full-domain Python control launch.
    #[must_use]
    pub const fn python(&self) -> &QualifiedNativeLaunch {
        &self.parts.python
    }
    /// Planning-domain GitHub launch, independent of the Full Mise selector set.
    #[must_use]
    pub const fn github(&self) -> &QualifiedNativeLaunch {
        &self.parts.github
    }
    /// Complete native environment, with Full homes but no compiler selector or MBX override.
    #[must_use]
    pub const fn environment(&self) -> &BTreeMap<String, String> {
        &self.parts.environment
    }
    /// Full Mise footprint; GitHub never becomes a Full selector.
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
    fn unmeasured_linux_control_never_chooses_a_fallback_host() {
        let catalog = ToolCatalog::pinned();
        for host in [DistributionHost::LinuxAmd64, DistributionHost::LinuxArm64] {
            assert!(qualified_source_snapshot_context(&catalog, host).is_err());
        }
    }

    #[test]
    fn missing_owned_mise_publication_never_grants_mac_control() {
        assert!(qualified_source_snapshot_context(
            &ToolCatalog::pinned(), DistributionHost::MacosArm64,
        ).is_err());
    }
}
