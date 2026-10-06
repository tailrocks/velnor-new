//! Independent validator jobs (P05-6: no Policy umbrella).
//!
//! Kinds, stable IDs, display names, and the consumer verification
//! selection surface (`[workflow.verify]`).

use serde::{Deserialize, Serialize};

/// Independent validator jobs (P05-6: no Policy umbrella).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorKind {
    /// Repository-structure lint.
    Alint,
    /// Dependency/security audit.
    CargoDeny,
    /// Unused-dependency scan.
    CargoMachete,
    /// Workflow-file lint.
    Actionlint,
    /// Workflow security audit.
    Zizmor,
    /// Markdown lint over repository docs.
    Markdownlint,
    /// Strict JSON syntax plus duplicate-key rejection.
    StrictJson,
    /// Skill frontmatter and ID agreement (Agent Skills spec).
    FrontmatterId,
    /// Markdown link checking.
    LinkCheck,
    /// Host-native plugin/skill validators, where installed.
    NativeValidators,
}

impl ValidatorKind {
    /// Stable unbranded job ID.
    #[must_use]
    pub fn job_id(&self) -> &'static str {
        match self {
            Self::Alint => "alint",
            Self::CargoDeny => "cargo-deny",
            Self::CargoMachete => "cargo-machete",
            Self::Actionlint => "actionlint",
            Self::Zizmor => "zizmor",
            Self::Markdownlint => "markdownlint",
            Self::StrictJson => "strict-json",
            Self::FrontmatterId => "frontmatter-id",
            Self::LinkCheck => "link-check",
            Self::NativeValidators => "native-validators",
        }
    }

    /// Stable human-readable display name.
    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Alint => "Alint",
            Self::CargoDeny => "Cargo Deny",
            Self::CargoMachete => "Cargo Machete",
            Self::Actionlint => "Actionlint",
            Self::Zizmor => "Zizmor",
            Self::Markdownlint => "Markdownlint",
            Self::StrictJson => "Strict JSON",
            Self::FrontmatterId => "Frontmatter ID",
            Self::LinkCheck => "Link Check",
            Self::NativeValidators => "Native Validators",
        }
    }

    /// Every validator kind in emission order.
    #[must_use]
    pub fn all() -> [Self; 10] {
        [
            Self::Alint,
            Self::CargoDeny,
            Self::CargoMachete,
            Self::Actionlint,
            Self::Zizmor,
            Self::Markdownlint,
            Self::StrictJson,
            Self::FrontmatterId,
            Self::LinkCheck,
            Self::NativeValidators,
        ]
    }

    /// Velnor-repository validators emitted as support jobs.
    ///
    /// Actionlint is always-on base IR on both policies, never support.
    #[must_use]
    pub fn repository_validators() -> [Self; 4] {
        [
            Self::Alint,
            Self::CargoDeny,
            Self::CargoMachete,
            Self::Zizmor,
        ]
    }
}

impl ValidatorKind {
    /// Consumer-selectable validators, in canonical emission order.
    ///
    /// The `[workflow.verify]` allowlist names a subset of these by job
    /// ID; the renderer merges them as support jobs on any policy.
    /// Cargo-backed validators stay Velnor-only: their vectors assume
    /// the Velnor workspace layout.
    #[must_use]
    pub fn consumer_verify() -> [Self; 7] {
        [
            Self::Zizmor,
            Self::Alint,
            Self::Markdownlint,
            Self::StrictJson,
            Self::FrontmatterId,
            Self::LinkCheck,
            Self::NativeValidators,
        ]
    }

    /// Resolve a `[workflow.verify]` job name to its validator kind.
    ///
    /// Names are the stable job IDs; anything else (including the
    /// Velnor-only and always-on IDs) resolves to `None` and fails
    /// config validation closed.
    #[must_use]
    pub fn from_verify_name(name: &str) -> Option<Self> {
        Self::consumer_verify()
            .into_iter()
            .find(|kind| kind.job_id() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_names_resolve_exactly() {
        for kind in ValidatorKind::consumer_verify() {
            assert_eq!(ValidatorKind::from_verify_name(kind.job_id()), Some(kind));
        }
        for name in ["cargo-deny", "cargo-machete", "actionlint", "plan", ""] {
            assert_eq!(ValidatorKind::from_verify_name(name), None, "{name}");
        }
    }
}
