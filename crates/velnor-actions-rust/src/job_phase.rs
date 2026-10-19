//! Fixed Rust execution ownership under the five-minute job ceiling.

use velnor_actions_contract::ContractError;

use crate::TaskKind;

/// Closed capacity phases; task identities and configurations remain unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RustJobPhase {
    /// Formatting and Clippy admission.
    Lint,
    /// Test compilation, execution, and documentation tests.
    Tests,
    /// Documentation generation.
    Docs,
}

impl RustJobPhase {
    /// Resolve one known Rust task kind to its sole execution phase.
    ///
    /// # Errors
    ///
    /// Rejects unknown task kinds rather than silently omitting obligations.
    pub fn from_task_kind(value: &str) -> Result<Self, ContractError> {
        Ok(match TaskKind::parse(value)? {
            TaskKind::Fmt | TaskKind::Clippy => Self::Lint,
            TaskKind::Build | TaskKind::Test | TaskKind::Nextest | TaskKind::Doctest => Self::Tests,
            TaskKind::Doc => Self::Docs,
        })
    }

    /// Stable job-identity suffix, independent of task configuration.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lint => "lint",
            Self::Tests => "tests",
            Self::Docs => "docs",
        }
    }

    /// Human-readable phase label.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Lint => "Lint",
            Self::Tests => "Tests",
            Self::Docs => "Documentation",
        }
    }
}

#[cfg(test)]
#[path = "job_phase_tests.rs"]
mod tests;
