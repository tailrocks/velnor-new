//! Closed provider and engine roles backed by the sole distribution registry.

use super::{
    PinnedTool,
    qualification::{
        DistributionHost, DistributionTool, GRADLE_SELECTION_VERSION, JAVA_SELECTION_VERSION,
        QualifiedDistribution,
    },
};
use crate::MiseError;

/// Provider whose Java distribution and launch closure have been audited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaProvider {
    /// Community `GraalVM` JDK, distinct from Oracle `GraalVM` binaries.
    GraalvmCommunity,
}

/// Native tool role; managed Gradle never selects a consumer wrapper engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeToolProfile {
    /// Provider-qualified Java runtime.
    Java(JavaProvider),
    /// Standalone catalog Gradle distribution.
    ManagedGradle,
}

impl NativeToolProfile {
    /// Select the closed native provider role, where applicable.
    #[must_use]
    pub const fn for_tool(tool: PinnedTool) -> Option<Self> {
        match tool {
            PinnedTool::Java => Some(Self::Java(JavaProvider::GraalvmCommunity)),
            PinnedTool::Gradle => Some(Self::ManagedGradle),
            _ => None,
        }
    }

    /// Exact selector version owned by the distribution registry.
    #[must_use]
    pub const fn selection_version(self) -> &'static str {
        match self {
            Self::Java(JavaProvider::GraalvmCommunity) => JAVA_SELECTION_VERSION,
            Self::ManagedGradle => GRADLE_SELECTION_VERSION,
        }
    }

    /// Resolve the exact installed distribution for this host and role.
    /// # Errors
    /// Rejects hosts lacking measured installation and launch qualification.
    pub fn distribution(self, host: DistributionHost) -> Result<QualifiedDistribution, MiseError> {
        QualifiedDistribution::require_native(self.tool(), host, self.selection_version())
    }

    /// Exact installation and execution selector for the measured host.
    /// # Errors
    /// Rejects missing installed distribution qualification.
    pub fn selector(self, host: DistributionHost) -> Result<&'static str, MiseError> {
        Ok(self.distribution(host)?.selector())
    }

    const fn tool(self) -> DistributionTool {
        match self {
            Self::Java(JavaProvider::GraalvmCommunity) => DistributionTool::Java,
            Self::ManagedGradle => DistributionTool::Gradle,
        }
    }
}
