//! Typed hosted qualification phases and their cache permissions.

use serde::{Deserialize, Serialize};

/// `$GITHUB_OUTPUT` name for the validated qualification namespace.
pub const QUALIFICATION_CAMPAIGN_OUTPUT: &str = "qualification_campaign";
/// `$GITHUB_OUTPUT` name for the validated qualification phase.
pub const QUALIFICATION_PHASE_OUTPUT: &str = "qualification_phase";
/// `$GITHUB_OUTPUT` name for qualification cache-reader eligibility.
pub const QUALIFICATION_CACHE_ENABLED_OUTPUT: &str = "qualification_cache_enabled";
/// `$GITHUB_OUTPUT` name for qualification cache-writer eligibility.
pub const QUALIFICATION_CACHE_WRITE_OUTPUT: &str = "qualification_cache_write";

/// Controlled hosted qualification phase supplied to `workflow_dispatch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationPhase {
    /// First run in an isolated campaign namespace; must prove a cold miss.
    Cold,
    /// Fresh-runner exact-snapshot reuse and persistence runs.
    Warm,
    /// Third unchanged run proving the late state persisted without rewriting it.
    Third,
    /// Separate experiment that adds a useful compiler-state delta.
    UsefulDelta,
    /// Same source and workload with all cache layers disabled.
    Control,
}

impl QualificationPhase {
    /// Canonical workflow-dispatch spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
            Self::Third => "third",
            Self::UsefulDelta => "useful_delta",
            Self::Control => "control",
        }
    }

    /// Whether this phase may restore an isolated campaign cache.
    #[must_use]
    pub const fn cache_enabled(self) -> bool {
        !matches!(self, Self::Control)
    }

    /// Whether this phase may publish an isolated campaign cache successor.
    #[must_use]
    pub const fn cache_write_allowed(self) -> bool {
        matches!(self, Self::Cold | Self::Warm | Self::UsefulDelta)
    }

    /// The immediately preceding phase required to advance this lineage.
    #[must_use]
    pub const fn predecessor(self) -> Option<Self> {
        match self {
            Self::Cold | Self::Control => None,
            Self::Warm => Some(Self::Cold),
            Self::Third => Some(Self::Warm),
            Self::UsefulDelta => Some(Self::Third),
        }
    }
}
