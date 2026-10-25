use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowLintConfig {
    #[serde(default)]
    pub profile: WorkflowLintProfile,
}

impl WorkflowLintConfig {
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowLintProfile {
    #[default]
    Correctness,

    Default,

    Pedantic,
}

impl WorkflowLintProfile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Correctness => "correctness",
            Self::Default => "default",
            Self::Pedantic => "pedantic",
        }
    }
}
