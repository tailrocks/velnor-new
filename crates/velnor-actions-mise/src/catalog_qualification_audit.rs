//! Source artifact auditing grants no native installation or selector capability.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, QualifiedDistribution,
    QualifiedLaunchKind, QualifiedSourceLineage,
};

/// Immutable source/asset evidence; cannot authorize an installed selector or path.
///
/// ```compile_fail
/// use velnor_actions_mise::catalog::qualification::NativeDistributionAudit;
/// fn install_from_audit(audit: NativeDistributionAudit) {
///     let _selector = audit.selector();
/// }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct NativeDistributionAudit {
    distribution: QualifiedDistribution,
}

impl std::fmt::Debug for NativeDistributionAudit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeDistributionAudit")
            .field("tool", &self.tool())
            .field("host", &self.host())
            .field("version", &self.version())
            .field("selection_version", &self.selection_version())
            .field("archive_sha256", &self.archive_sha256())
            .finish_non_exhaustive()
    }
}

/// Archive launch bytes without any installed location or execution capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditedLaunchEntry {
    archive_member: &'static str,
    sha256: &'static str,
    kind: QualifiedLaunchKind,
}

impl NativeDistributionAudit {
    pub(super) const fn new(distribution: QualifiedDistribution) -> Self {
        Self { distribution }
    }
    /// Audited tool.
    #[must_use]
    pub const fn tool(&self) -> DistributionTool {
        self.distribution.tool
    }
    /// Audited host.
    #[must_use]
    pub const fn host(&self) -> DistributionHost {
        self.distribution.host
    }
    /// Audited asset url.
    #[must_use]
    pub const fn asset_url(&self) -> &'static str {
        self.distribution.asset_url
    }
    /// Audited archive sha256.
    #[must_use]
    pub const fn archive_sha256(&self) -> &'static str {
        self.distribution.archive_sha256
    }
    /// Audited binary sha256.
    #[must_use]
    pub const fn binary_sha256(&self) -> &'static str {
        self.distribution.binary_sha256
    }
    /// Audited asset format.
    #[must_use]
    pub const fn asset_format(&self) -> DistributionAssetFormat {
        self.distribution.asset_format
    }
    /// Audited binary member.
    #[must_use]
    pub const fn binary_member(&self) -> &'static str {
        self.distribution.binary_member
    }
    /// Audited source repository.
    #[must_use]
    pub const fn source_repository(&self) -> &'static str {
        self.distribution.source_repository
    }
    /// Audited source commit.
    #[must_use]
    pub const fn source_commit(&self) -> &'static str {
        self.distribution.source_commit
    }
    /// Audited source tree.
    #[must_use]
    pub const fn source_tree(&self) -> &'static str {
        self.distribution.source_tree
    }
    /// Audited owner.
    #[must_use]
    pub const fn owner(&self) -> &'static str {
        self.distribution.owner
    }
    /// Audited version.
    #[must_use]
    pub const fn version(&self) -> &'static str {
        self.distribution.version
    }
    /// Audited selection version.
    #[must_use]
    pub const fn selection_version(&self) -> &'static str {
        self.distribution.selection_version
    }
    /// Audited abi.
    #[must_use]
    pub const fn abi(&self) -> &'static str {
        self.distribution.abi
    }
    /// Canonical complete record identity; carries no installed selector capability.
    #[must_use]
    pub fn qualification_digest(&self) -> String {
        self.distribution.qualification_digest()
    }
    /// Source archive launch inputs; installed locations are deliberately unavailable.
    #[must_use]
    pub fn launch_entries(&self) -> Vec<AuditedLaunchEntry> {
        self.distribution
            .launch_entries
            .iter()
            .map(|entry| AuditedLaunchEntry {
                archive_member: entry.archive_member,
                sha256: entry.sha256,
                kind: entry.kind,
            })
            .collect()
    }
    /// Immutable source lineage independent of installation qualification.
    #[must_use]
    pub const fn source_lineage(&self) -> &'static [QualifiedSourceLineage] {
        self.distribution.source_lineage
    }
}

impl AuditedLaunchEntry {
    /// Exact regular source archive member.
    #[must_use]
    pub const fn archive_member(&self) -> &'static str {
        self.archive_member
    }
    /// Expected source archive member SHA256.
    #[must_use]
    pub const fn sha256(&self) -> &'static str {
        self.sha256
    }
    /// Source byte execution role.
    #[must_use]
    pub const fn kind(&self) -> QualifiedLaunchKind {
        self.kind
    }
}
