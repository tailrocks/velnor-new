//! Pinned actionlint version and syntax-capability flags.
//!
//! The capability struct records exactly which workflow syntax the pinned
//! release parses and validates. Matrix/job parallelism is supported. The
//! pinned release does not parse GitHub's native step concurrency keys
//! (rhysd/actionlint#693), so Velnor emits only the `background: true` plus
//! `wait-all` spelling and runs actionlint with the two exact
//! [`STEP_PARALLELISM_IGNORES`]: every background member is still parsed and
//! validated (keys, expressions, action inputs, `ShellCheck`), and only the
//! unknown `background` key and the body-less join step are suppressed.

use crate::ActionlintError;

/// Pinned actionlint release (verified 2026-09-28 per version policy).
pub const ACTIONLINT_VERSION: &str = "1.7.12";

/// Exact actionlint `-ignore` message patterns bridging native step
/// concurrency for releases that do not parse it.
///
/// The first matches only the unknown `background` key on a `run` or `uses`
/// step; the member's remaining keys are still checked. The second matches
/// the generated `wait-all` join, which by GitHub's grammar has no `run` or
/// `uses`. Every other generated step is typed to render `run` or `uses`, and
/// GitHub itself rejects a hand-written step without either at run time.
pub const STEP_PARALLELISM_IGNORES: [&str; 2] = [
    r#"^unexpected key "background" for step to (run shell command|execute action)\. "#,
    r#"^step must run script with "run" section or run action with "uses" section$"#,
];

/// Workflow syntax a renderer proposes to emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepSyntax {
    /// Independent jobs plus `strategy.matrix` fan-out.
    JobMatrix,
    /// Native `parallel:`/`background:`/`wait` step keys.
    NativeParallelism,
}

/// Capability flags for one exact actionlint release.
///
/// Construct with [`ActionlintCapabilities::for_pinned`]; every broader
/// capability needs an explicit qualifier call so default construction
/// can never claim unqualified syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "spec-mandated capability flag struct"
)]
pub struct ActionlintCapabilities {
    /// Parses `strategy.matrix` (plus `fail-fast`, `max-parallel`).
    matrix_strategy: bool,
    /// Validates concurrent matrix/job fan-out.
    job_parallelism: bool,
    /// Parses native `parallel:`/`background:`/`wait` step keys.
    native_step_parallelism: bool,
    /// Native step concurrency is emitted through [`STEP_PARALLELISM_IGNORES`].
    step_parallelism_bridge: bool,
    /// Parses workflow-level and job-level `cache-mode` keys (2026-09-10).
    native_cache_mode: bool,
    /// Built-in hosted-label table contains `ubuntu-26.04`.
    recognizes_ubuntu_26_04_hosted_label: bool,
}

impl ActionlintCapabilities {
    /// Exact capabilities of [`ACTIONLINT_VERSION`].
    #[must_use]
    pub const fn for_pinned() -> Self {
        Self {
            matrix_strategy: true,
            job_parallelism: true,
            native_step_parallelism: false,
            step_parallelism_bridge: true,
            native_cache_mode: false,
            recognizes_ubuntu_26_04_hosted_label: false,
        }
    }

    /// Whether `strategy.matrix` syntax is supported.
    #[must_use]
    pub const fn supports_matrix_strategy(self) -> bool {
        self.matrix_strategy
    }

    /// Whether concurrent job/matrix fan-out is supported.
    #[must_use]
    pub const fn supports_job_parallelism(self) -> bool {
        self.job_parallelism
    }

    /// Whether native step parallelism is qualified for emission.
    #[must_use]
    pub const fn native_step_parallelism_qualified(self) -> bool {
        self.native_step_parallelism || self.step_parallelism_bridge
    }

    /// Whether lint invocations must pass [`STEP_PARALLELISM_IGNORES`].
    #[must_use]
    pub const fn requires_step_parallelism_bridge(self) -> bool {
        !self.native_step_parallelism && self.step_parallelism_bridge
    }

    /// Whether native `cache-mode` syntax (2026-09-10) is supported and qualified.
    #[must_use]
    pub const fn supports_native_cache_mode(self) -> bool {
        self.native_cache_mode
    }

    /// Whether native `cache-mode` emission is qualified.
    #[must_use]
    pub const fn native_cache_mode_qualified(self) -> bool {
        self.native_cache_mode
    }

    /// Whether the pinned release natively recognizes `ubuntu-26.04`.
    #[must_use]
    pub const fn recognizes_ubuntu_26_04_hosted_label(self) -> bool {
        self.recognizes_ubuntu_26_04_hosted_label
    }

    /// Record that a newer pinned release parses native step concurrency.
    ///
    /// Removes the ignore bridge. Only valid together with an
    /// `ACTIONLINT_VERSION` bump, never alone.
    #[must_use]
    pub const fn parse_native_step_parallelism(mut self) -> Self {
        self.native_step_parallelism = true;
        self
    }

    /// Qualify workflow-level and job-level `cache-mode` emission once a
    /// newer pinned `actionlint` release recognizes `cache-mode` (2026-09-10).
    #[must_use]
    pub const fn qualify_native_cache_mode(mut self) -> Self {
        self.native_cache_mode = true;
        self
    }

    /// Record that a newer pinned release recognizes `ubuntu-26.04`.
    ///
    /// Removes the runner-label bridge from generated config. Only valid
    /// together with an `ACTIONLINT_VERSION` bump, never alone.
    #[must_use]
    pub const fn recognize_hosted_label_26_04(mut self) -> Self {
        self.recognizes_ubuntu_26_04_hosted_label = true;
        self
    }

    /// Whether generated config must carry the runner-label bridge.
    #[must_use]
    pub const fn requires_runner_label_bridge(self) -> bool {
        !self.recognizes_ubuntu_26_04_hosted_label
    }

    /// Reject `cache-mode` emission when the pinned `actionlint` release has
    /// not yet been qualified for `cache-mode` syntax.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError::UnsupportedSyntax`] when `native_cache_mode`
    /// is unqualified.
    pub const fn check_native_cache_mode(self) -> Result<(), ActionlintError> {
        if self.native_cache_mode {
            Ok(())
        } else {
            Err(ActionlintError::UnsupportedSyntax {
                syntax: "cache_mode",
            })
        }
    }

    /// Reject syntax the pinned release cannot parse and validate.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError::UnsupportedSyntax`] when native step
    /// parallelism is requested without qualification.
    pub const fn check_step_syntax(self, syntax: StepSyntax) -> Result<(), ActionlintError> {
        match syntax {
            StepSyntax::JobMatrix => {
                if self.matrix_strategy && self.job_parallelism {
                    Ok(())
                } else {
                    Err(ActionlintError::UnsupportedSyntax {
                        syntax: "job_matrix",
                    })
                }
            }
            StepSyntax::NativeParallelism => {
                if self.native_step_parallelism_qualified() {
                    Ok(())
                } else {
                    Err(ActionlintError::UnsupportedSyntax {
                        syntax: "native_parallelism",
                    })
                }
            }
        }
    }
}

impl Default for ActionlintCapabilities {
    fn default() -> Self {
        Self::for_pinned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_actionlint_bridges_parallelism_and_defaults_cache_mode_to_false() {
        let caps = ActionlintCapabilities::for_pinned();
        assert!(caps.supports_matrix_strategy());
        assert!(caps.supports_job_parallelism());
        assert!(caps.native_step_parallelism_qualified());
        assert!(caps.requires_step_parallelism_bridge());
        assert!(
            !caps
                .parse_native_step_parallelism()
                .requires_step_parallelism_bridge()
        );
        assert!(!caps.supports_native_cache_mode());
        assert!(!caps.native_cache_mode_qualified());
        assert_eq!(
            caps.check_native_cache_mode(),
            Err(ActionlintError::UnsupportedSyntax {
                syntax: "cache_mode"
            })
        );
        let qualified = caps.qualify_native_cache_mode();
        assert!(qualified.supports_native_cache_mode());
        assert!(qualified.native_cache_mode_qualified());
        assert_eq!(qualified.check_native_cache_mode(), Ok(()));
    }
}
