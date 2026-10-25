use serde::{Deserialize, Serialize};

pub const QUALIFICATION_CAMPAIGN_OUTPUT: &str = "qualification_campaign";

pub const QUALIFICATION_PHASE_OUTPUT: &str = "qualification_phase";

pub const QUALIFICATION_CACHE_ENABLED_OUTPUT: &str = "qualification_cache_enabled";

pub const QUALIFICATION_CACHE_WRITE_OUTPUT: &str = "qualification_cache_write";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationPhase {
    Cold,

    Warm,

    Third,

    UsefulDelta,

    Control,
}

impl QualificationPhase {
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

    #[must_use]
    pub const fn cache_enabled(self) -> bool {
        !matches!(self, Self::Control)
    }

    #[must_use]
    pub const fn cache_write_allowed(self) -> bool {
        matches!(self, Self::Cold | Self::Warm | Self::UsefulDelta)
    }

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
