//! Typed GitHub Actions step output identities.

use serde::{Deserialize, Serialize};

/// Plan-job dependency seed lookup ids, one per workspace root ordinal.
const RSDEPS_LOOKUP_IDS: [&str; super::cache_family::MAX_RSDEPS_ROOTS] = [
    "rsdeps-lookup-0",
    "rsdeps-lookup-1",
    "rsdeps-lookup-2",
    "rsdeps-lookup-3",
    "rsdeps-lookup-4",
    "rsdeps-lookup-5",
    "rsdeps-lookup-6",
    "rsdeps-lookup-7",
    "rsdeps-lookup-8",
    "rsdeps-lookup-9",
    "rsdeps-lookup-10",
    "rsdeps-lookup-11",
    "rsdeps-lookup-12",
    "rsdeps-lookup-13",
    "rsdeps-lookup-14",
    "rsdeps-lookup-15",
];

/// Stable GitHub Actions id for a step whose outputs or outcome have consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepId {
    /// Planner output consumed by downstream jobs.
    Plan,
    /// V2 hosted tools-cache output consumed by restore and save.
    ToolsCacheIdentity,
    /// `OpenTofu` provider-cache composite outputs consumed by the save step.
    TofuProviders,
    /// Runtime cache trust and family keys consumed by restores and saves.
    CacheKeys,
    /// Plan-job sources restore whose hit outputs gate the sources save.
    SourcesRestore,
    /// Last Plan-job fetch step whose outcome gates the sources save.
    SourcesFetch,
    /// V2 tools restore whose exact-hit output gates the tools save.
    ToolsRestore,
    /// Plan-job exact-key probe of one root's dependency seed (`< 16`).
    RsdepsLookup(u8),
    /// Cache seed exact-key probe of the job root's dependency seed.
    SeedLookup,
    /// Cache seed exact-key probe of the helper dependency seed.
    SeedHelperLookup,
    /// Cache seed build lane.
    SeedBuild,
    /// Cache seed Clippy lane.
    SeedClippy,
    /// Cache seed documentation lane.
    SeedDoc,
    /// Cache seed helper lane (bootstrap Plan helper build).
    SeedHelper,
    /// Cache seed trim whose outputs gate the saves.
    SeedTrim,
    /// Cache seed dependency seed save, gating its janitor.
    SeedSave,
    /// Cache seed helper dependency seed save, gating its janitor.
    SeedHelperSave,
}

impl StepId {
    /// Fixed YAML spelling for this step output owner.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::ToolsCacheIdentity => "v2",
            Self::TofuProviders => "tofu-providers",
            Self::CacheKeys => "cache-keys",
            Self::SourcesRestore => "sources-restore",
            Self::SourcesFetch => "sources-fetch",
            Self::ToolsRestore => "tools-restore",
            Self::RsdepsLookup(ordinal) => {
                let at = ordinal as usize;
                if at < RSDEPS_LOOKUP_IDS.len() {
                    RSDEPS_LOOKUP_IDS[at]
                } else {
                    "rsdeps-lookup-out-of-range"
                }
            }
            Self::SeedLookup => "seed-lookup",
            Self::SeedHelperLookup => "seed-helper-lookup",
            Self::SeedBuild => "seed-build",
            Self::SeedClippy => "seed-clippy",
            Self::SeedDoc => "seed-doc",
            Self::SeedHelper => "seed-helper",
            Self::SeedTrim => "seed-trim",
            Self::SeedSave => "seed-save",
            Self::SeedHelperSave => "seed-helper-save",
        }
    }

    /// The Plan lookup id of root `ordinal`, when it is in range.
    #[must_use]
    pub fn rsdeps_lookup(ordinal: usize) -> Option<Self> {
        u8::try_from(ordinal)
            .ok()
            .filter(|_| ordinal < RSDEPS_LOOKUP_IDS.len())
            .map(Self::RsdepsLookup)
    }

    /// True when a typed id names an existing spelling.
    #[must_use]
    pub const fn in_range(self) -> bool {
        match self {
            Self::RsdepsLookup(ordinal) => (ordinal as usize) < RSDEPS_LOOKUP_IDS.len(),
            _ => true,
        }
    }
}
