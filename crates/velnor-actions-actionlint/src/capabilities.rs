//! Pinned jactionlint version and the workflow syntax it validates.
//!
//! jactionlint 2.3.0 parses and validates everything Velnor emits:
//! `strategy.matrix`, native step concurrency (`background: true` plus the
//! body-less `wait-all` join), workflow- and job-level `cache-mode`, and the
//! `ubuntu-26.04` hosted label. No ignore pattern or label bridge exists;
//! the only runner labels Velnor declares are the ones the linter cannot
//! know ([`is_known_hosted_label`]).

use crate::ActionlintError;

/// Pinned jactionlint release (mirrors the Mise catalog pin, checked by the
/// freshness gate).
pub const JACTIONLINT_VERSION: &str = "2.3.0";

/// GitHub-hosted labels the pinned jactionlint recognises natively, limited
/// to the labels Velnor can emit (`RUNNER_LABEL_CATALOG` and the release
/// target labels). Read from jactionlint 2.3.0's `unknown-runner-label`
/// message, which lists its whole built-in table.
const KNOWN_HOSTED_LABELS: [&str; 13] = [
    "macos-14",
    "macos-15",
    "macos-15-intel",
    "macos-26",
    "macos-26-intel",
    "ubuntu-22.04",
    "ubuntu-22.04-arm",
    "ubuntu-24.04",
    "ubuntu-24.04-arm",
    "ubuntu-26.04",
    "ubuntu-26.04-arm",
    "ubuntu-latest",
    "xcode-27",
];

/// Whether the pinned jactionlint knows `label` as a GitHub-hosted runner.
///
/// Such a label must never be declared under `self-hosted-runner`: the
/// linter would then treat jobs on GitHub's runners as self-hosted.
#[must_use]
pub fn is_known_hosted_label(label: &str) -> bool {
    KNOWN_HOSTED_LABELS.contains(&label)
}

/// Workflow syntax a renderer proposes to emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepSyntax {
    /// Independent jobs plus `strategy.matrix` fan-out.
    JobMatrix,
    /// Native `background:` steps joined by `wait-all`.
    NativeParallelism,
    /// Workflow-level and job-level `cache-mode`.
    CacheMode,
}

/// What the pinned jactionlint release validates.
///
/// Construct with [`JactionlintCapabilities::for_pinned`]. A future pin that
/// lost a capability would clear its flag here, and generation would then
/// refuse that syntax instead of emitting what the linter cannot check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JactionlintCapabilities {
    /// Parses and validates `strategy.matrix` and job fan-out.
    job_matrix: bool,
    /// Parses and validates `background:` steps and the `wait-all` join.
    native_step_parallelism: bool,
    /// Parses and validates `cache-mode` literals.
    cache_mode: bool,
}

impl JactionlintCapabilities {
    /// Exact capabilities of [`JACTIONLINT_VERSION`].
    #[must_use]
    pub const fn for_pinned() -> Self {
        Self {
            job_matrix: true,
            native_step_parallelism: true,
            cache_mode: true,
        }
    }

    /// Reject syntax the pinned release cannot parse and validate.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError::UnsupportedSyntax`] for unqualified syntax.
    pub const fn check_step_syntax(self, syntax: StepSyntax) -> Result<(), ActionlintError> {
        let (supported, name) = match syntax {
            StepSyntax::JobMatrix => (self.job_matrix, "job_matrix"),
            StepSyntax::NativeParallelism => (self.native_step_parallelism, "native_parallelism"),
            StepSyntax::CacheMode => (self.cache_mode, "cache_mode"),
        };
        if supported {
            Ok(())
        } else {
            Err(ActionlintError::UnsupportedSyntax { syntax: name })
        }
    }
}

impl Default for JactionlintCapabilities {
    fn default() -> Self {
        Self::for_pinned()
    }
}

#[cfg(test)]
mod tests {
    use super::{JactionlintCapabilities, StepSyntax, is_known_hosted_label};

    #[test]
    fn pinned_jactionlint_validates_everything_velnor_emits() {
        let caps = JactionlintCapabilities::for_pinned();
        for syntax in [
            StepSyntax::JobMatrix,
            StepSyntax::NativeParallelism,
            StepSyntax::CacheMode,
        ] {
            assert_eq!(caps.check_step_syntax(syntax), Ok(()), "{syntax:?}");
        }
    }

    #[test]
    fn hosted_labels_are_known_and_custom_labels_are_not() {
        for label in [
            "ubuntu-26.04",
            "ubuntu-24.04-arm",
            "macos-15",
            "macos-26-intel",
        ] {
            assert!(is_known_hosted_label(label), "{label}");
        }
        for label in ["velnor", "ubuntu-26.04-scale-set", "macos-15-arm64", ""] {
            assert!(!is_known_hosted_label(label), "{label}");
        }
    }
}
