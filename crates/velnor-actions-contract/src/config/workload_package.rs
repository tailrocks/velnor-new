//! Closed package scripts and their compiled execution order.

use super::{WorkloadConfig, WorkloadKind};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Reviewed package script names; arbitrary package commands are forbidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageScript {
    /// Static lint policy.
    Lint,
    /// Type correctness.
    Typecheck,
    /// Package-owned aggregate correctness gate.
    Check,
    /// Build package outputs.
    Build,
    /// Package tests.
    Test,
}

impl PackageScript {
    /// Fixed script spelling, also the unique proposal phase.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Lint => "lint",
            Self::Typecheck => "typecheck",
            Self::Check => "check",
            Self::Build => "build",
            Self::Test => "test",
        }
    }

    /// Canonical package phase rank shared by validation and rendering.
    #[must_use]
    pub const fn rank(self) -> u32 {
        match self {
            Self::Lint => 1,
            Self::Typecheck => 2,
            Self::Check => 3,
            Self::Build => 4,
            Self::Test => 5,
        }
    }
}

impl WorkloadConfig {
    /// Explicit scripts, or the established Bun build/test contract.
    #[must_use]
    pub fn package_scripts(&self) -> &[PackageScript] {
        self.scripts
            .as_deref()
            .unwrap_or(&[PackageScript::Build, PackageScript::Test])
    }

    /// Validate script applicability and canonical unique ordering.
    /// # Errors
    /// Reports missing, empty, duplicate, unordered or inapplicable scripts.
    pub(super) fn validate_package(&self, file: &str) -> Result<(), ContractError> {
        let key = format!("stacks.workloads.{}.scripts", self.name);
        let problem = match (self.kind, self.scripts.as_deref()) {
            (WorkloadKind::NodeCi, None) => Some("explicit_scripts_required"),
            (WorkloadKind::BunCi | WorkloadKind::NodeCi, Some([])) => Some("empty_scripts"),
            (WorkloadKind::BunCi | WorkloadKind::NodeCi, Some(scripts)) => {
                if scripts.windows(2).any(|pair| pair[0] == pair[1]) {
                    Some("duplicate_script")
                } else if scripts
                    .windows(2)
                    .any(|pair| pair[0].rank() > pair[1].rank())
                {
                    Some("must_be_sorted")
                } else {
                    None
                }
            }
            (_, None) => None,
            (_, Some(_)) => Some("unexpected_scripts"),
        };
        if let Some(problem) = problem {
            return Err(ContractError::config(file, key, problem));
        }
        Ok(())
    }
}
