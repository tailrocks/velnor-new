//! Closed cache payload ownership shared by factories and workflow contracts.

use std::collections::BTreeMap;

use super::tool_producer::ToolCacheDomain;

/// Cache owners supported by the fixed opaque filesystem observer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheSnapshotDomain {
    /// Complete isolated Mise/Rustup executable installation.
    Tools,
    /// Small Gh and validator installation; no Rust or Cargo payload.
    PlanningTools,
    /// Node executable bootstrap for public npm source production.
    NpmBootstrap,
    /// Bun executable bootstrap for public source production.
    BunBootstrap,
    /// `OpenTofu` executable bootstrap for provider production.
    TofuBootstrap,
    /// Java executable bootstrap for Gradle source production.
    GradleBootstrap,
    /// Credential-free Cargo registry archives/index and Git object databases.
    Sources,
    /// Bun downloads; repository caches and temporary credential state excluded.
    BunDownloads,
    /// npm public download content; excludes logs and ambient configuration.
    NpmDownloads,
    /// Gradle content-addressed dependency artifacts only.
    GradleDependencies,
}

impl CacheSnapshotDomain {
    /// Every supported owned cache payload domain.
    pub const ALL: [Self; 10] = [
        Self::Tools,
        Self::PlanningTools,
        Self::NpmBootstrap,
        Self::BunBootstrap,
        Self::TofuBootstrap,
        Self::GradleBootstrap,
        Self::Sources,
        Self::BunDownloads,
        Self::NpmDownloads,
        Self::GradleDependencies,
    ];

    /// Bind the closed executable owner to its canonical observer.
    #[must_use]
    pub const fn tool_domain(domain: ToolCacheDomain) -> Self {
        match domain {
            ToolCacheDomain::Planning => Self::PlanningTools,
            ToolCacheDomain::Full => Self::Tools,
            ToolCacheDomain::NpmBootstrap => Self::NpmBootstrap,
            ToolCacheDomain::BunBootstrap => Self::BunBootstrap,
            ToolCacheDomain::TofuBootstrap => Self::TofuBootstrap,
            ToolCacheDomain::GradleBootstrap => Self::GradleBootstrap,
        }
    }

    /// Stable name used only for owned bookkeeping and output bindings.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::PlanningTools => "planning_tools",
            Self::NpmBootstrap => "npm_bootstrap",
            Self::BunBootstrap => "bun_bootstrap",
            Self::TofuBootstrap => "tofu_bootstrap",
            Self::GradleBootstrap => "gradle_bootstrap",
            Self::Sources => "sources",
            Self::BunDownloads => "bun_downloads",
            Self::NpmDownloads => "npm_downloads",
            Self::GradleDependencies => "gradle_dependencies",
        }
    }

    /// Fixed canonical payload roots. No repository expression is accepted.
    #[must_use]
    pub fn roots(self) -> &'static [&'static str] {
        match self {
            Self::Tools => &[
                "mise",
                "rustup",
                "cargo/bin",
                "cargo/.crates.toml",
                "cargo/.crates2.json",
            ],
            Self::PlanningTools => &["planning/mise"],
            Self::NpmBootstrap => &["npm-source/mise"],
            Self::BunBootstrap => &["bun-source/mise"],
            Self::TofuBootstrap => &["tofu-provider-producer/mise"],
            Self::GradleBootstrap => &["gradle-source/mise"],
            Self::Sources => &[
                "cargo/registry/index",
                "cargo/registry/cache",
                "cargo/git/db",
            ],
            Self::BunDownloads => &["native/bun/install/cache"],
            Self::NpmDownloads => &[
                "native/npm/_cacache/content-v2",
                "native/npm/public-proof-v1.json",
            ],
            Self::GradleDependencies => &["native/gradle/caches/modules-2/files-2.1"],
        }
    }

    /// Fixed bindings for the selected observation phase and restore action.
    #[must_use]
    pub fn environment(self, before: bool) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("VELNOR_SNAPSHOT_LAYER".to_owned(), self.name().to_owned()),
            (
                "VELNOR_SNAPSHOT_PHASE".to_owned(),
                if before { "before" } else { "after" }.to_owned(),
            ),
            ("VELNOR_SNAPSHOT_ROOTS".to_owned(), self.roots().join(",")),
            (
                "VELNOR_SNAPSHOT_OUTPUT".to_owned(),
                self.name().to_ascii_uppercase(),
            ),
            (
                "VELNOR_SNAPSHOT_RESTORED".to_owned(),
                format!(
                    "${{{{steps.{}.outputs.cache-matched-key}}}}",
                    self.restore_id()
                ),
            ),
        ])
    }

    /// Canonical restore action evidence identifier.
    #[must_use]
    pub fn restore_id(self) -> &'static str {
        match self {
            Self::Tools => "velnor-tools-cache",
            Self::PlanningTools => "velnor-planning-tools-cache",
            Self::NpmBootstrap => "velnor-npm-bootstrap-cache",
            Self::BunBootstrap => "velnor-bun-bootstrap-cache",
            Self::TofuBootstrap => "velnor-tofu-bootstrap-cache",
            Self::GradleBootstrap => "velnor-gradle-bootstrap-cache",
            Self::Sources => "velnor-sources-cache",
            Self::BunDownloads => "velnor-bun-cache",
            Self::NpmDownloads => "velnor-npm-cache",
            Self::GradleDependencies => "velnor-gradle-dependencies-cache",
        }
    }
}
