use serde::{Deserialize, Serialize};

pub const GRADLE_RUN_OPERATION: &str = "gradle-run-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepId {
    Plan,

    ToolsCacheIdentity,

    TofuProviders,

    MbxReady,

    CacheKeys,

    SourcesRestore,

    SourcesFetch,

    ToolsRestore,

    DepsToken,

    MbxBundleKey,

    MbxBundleRestore,

    MbxBundleExport,

    FamilyKeys,

    FamilySizeBunInstall,

    FamilySizePlaywrightBrowsers,

    FamilySizeDbtPackages,

    FamilySizePythonVenv,

    FamilySizeAnsibleCollections,
}

impl StepId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::ToolsCacheIdentity => "v2",
            Self::TofuProviders => "tofu-providers",
            Self::MbxReady => "mbx-ready",
            Self::CacheKeys => "cache-keys",
            Self::SourcesRestore => "sources-restore",
            Self::SourcesFetch => "sources-fetch",
            Self::ToolsRestore => "tools-restore",
            Self::DepsToken => "deps-token",
            Self::MbxBundleKey => "mbx-bundle-key",
            Self::MbxBundleRestore => "mbx-bundle-restore",
            Self::MbxBundleExport => "mbx-bundle-export",
            Self::FamilyKeys => "family-keys",
            Self::FamilySizeBunInstall => "family-size-bun-install",
            Self::FamilySizePlaywrightBrowsers => "family-size-playwright-browsers",
            Self::FamilySizeDbtPackages => "family-size-dbt-packages",
            Self::FamilySizePythonVenv => "family-size-python-venv",
            Self::FamilySizeAnsibleCollections => "family-size-ansible-collections",
        }
    }
}
