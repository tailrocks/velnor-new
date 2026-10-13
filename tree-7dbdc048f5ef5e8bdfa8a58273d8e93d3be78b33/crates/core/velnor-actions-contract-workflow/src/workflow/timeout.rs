//! Per-job `timeout-minutes`: bounded CI jobs, never the 6 h default.
//!
//! A missing timeout lets a buggy or malicious PR burn GitHub's 360
//! minute default per job (G4). The bound is structural: [`JobTimeout`]
//! is a required [`crate::workflow::ir::Job`] field (and a required
//! renderer `ReleaseJobSpec` field), so every generated job carries
//! `timeout-minutes` by construction and the renderer emits it
//! unconditionally. Defaults come from the measured green run
//! in `docs/content/docs/implemented/performance.mdx` (run 36777030585), never from
//! guesswork; see each constant.

use serde::{Deserialize, Serialize};

use velnor_actions_contract::errors::ContractError;

/// Per-job timeout in whole minutes (GitHub `timeout-minutes`).
///
/// Private minutes: only [`Self::new`] (fallible) and the audited
/// per-kind constants below construct values. Serde derive bypasses
/// validation, so [`crate::workflow::ir::WorkflowIr::validate`]
/// re-checks every deserialized value through [`Self::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobTimeout(u16);

impl JobTimeout {
    /// Smallest meaningful bound: zero would fail every job instantly.
    pub const MIN_MINUTES: u16 = 1;
    /// Largest admissible bound: GitHub's own 360 minute default. A
    /// timeout above the default it exists to undercut is absurd.
    pub const MAX_MINUTES: u16 = 360;

    /// Planner job: measured 88 s (tools 15 s + Build helper 59 s),
    /// so 10 minutes is ~7x headroom for cold-install variance.
    pub const PLAN: Self = Self(10);
    /// Crate job: measured max ~13 minutes (orchestrator: 686 s tests
    /// plus setup), so 30 minutes is ~2.4x over the slowest suite.
    pub const CRATE: Self = Self(30);
    /// Required gate: measured 46 s (fetch-reports 33 s), so 10
    /// minutes is ~13x over the merge plus download path.
    pub const REQUIRED: Self = Self(10);
    /// Validator job: each measured at most 20 s (cold installs
    /// dominate), so 10 minutes bounds even the slowest analyzer.
    pub const VALIDATOR: Self = Self(10);
    /// Candidate job: unmeasured; build once plus qualify is
    /// crate-shaped, so it carries the conservative crate bound.
    pub const CANDIDATE: Self = Self(30);
    /// Release job: two `gh` steps, no compile; matches the small-job
    /// bound.
    pub const RELEASE: Self = Self(10);
    /// MSRV job: unmeasured; per-crate compile plus check is
    /// crate-shaped, so it carries the conservative crate bound.
    pub const MSRV: Self = Self(30);
    /// Publish job: acquire plus plan download plus baseline upload,
    /// no compile; matches the small-job bound.
    pub const PUBLISH: Self = Self(10);

    /// Every per-kind default, for the audit test below.
    #[cfg(test)]
    const DEFAULTS: [Self; 8] = [
        Self::PLAN,
        Self::CRATE,
        Self::REQUIRED,
        Self::VALIDATOR,
        Self::CANDIDATE,
        Self::RELEASE,
        Self::MSRV,
        Self::PUBLISH,
    ];

    /// Bound `minutes` to `1..=360`.
    /// # Errors
    pub fn new(minutes: u16) -> Result<Self, ContractError> {
        let timeout = Self(minutes);
        timeout.validate()?;
        Ok(timeout)
    }

    /// Re-check a deserialized value; the `Job` gate calls this.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.0 < Self::MIN_MINUTES || self.0 > Self::MAX_MINUTES {
            return Err(ContractError::identity(
                "job.timeout_minutes",
                format!("bad_timeout:{}", self.0),
            ));
        }
        Ok(())
    }

    /// Whole minutes for the renderer.
    #[must_use]
    pub fn minutes(&self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests;
