//! Pinned actionlint version and syntax-capability flags.
//!
//! The capability struct records exactly which workflow syntax the pinned
//! release parses and validates. Matrix/job parallelism is supported;
//! native `parallel:`/`background:`/`wait` step syntax stays unqualified
//! until a Gate 7 syntax-capability update qualifies it.

use crate::ActionlintError;

/// Pinned actionlint release (verified 2026-09-28 per version policy).
pub const ACTIONLINT_VERSION: &str = "1.7.12";

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
        self.native_step_parallelism
    }

    /// Whether the pinned release natively recognizes `ubuntu-26.04`.
    #[must_use]
    pub const fn recognizes_ubuntu_26_04_hosted_label(self) -> bool {
        self.recognizes_ubuntu_26_04_hosted_label
    }

    /// Qualify native `parallel:`/`background:`/`wait` emission.
    ///
    /// Gate 7 only: call after successful joins, failed children,
    /// cancellation, output visibility, and timing are qualified.
    #[must_use]
    pub const fn qualify_native_step_parallelism(mut self) -> Self {
        self.native_step_parallelism = true;
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
                if self.native_step_parallelism {
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
