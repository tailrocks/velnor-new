//! Per-job `timeout-minutes`: bounded CI jobs, never the 6 h default.
//!
//! A missing timeout lets a buggy or malicious PR burn GitHub's 360
//! minute default per job (G4). The bound is structural: [`JobTimeout`]
//! is a required [`crate::workflow::ir::Job`] field (and a required
//! renderer `ReleaseJobSpec` field), so every generated job carries
//! `timeout-minutes` by construction and the renderer emits it
//! unconditionally. Defaults come from the measured green run
//! in `docs/implemented/performance.md` (run 36777030585), never from
//! guesswork; see each constant.

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

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

    /// Planner job. The small measured path is 88 s. Java run
    /// 37346647634 spent 569 s in Checkout and the 10 minute bound
    /// cancelled the job during Fetch Cargo sources, so Plan never ran.
    /// Twenty minutes covers that checkout and the remaining plan steps.
    pub const PLAN: Self = Self(20);
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
mod tests {
    use super::JobTimeout;

    #[test]
    fn range_edges_hold_and_beyond_fails() {
        assert_eq!(JobTimeout::new(1).expect("fixture holds").minutes(), 1);
        assert_eq!(JobTimeout::new(360).expect("fixture holds").minutes(), 360);
        for bad in [0, 361, 1000, u16::MAX] {
            let err = JobTimeout::new(bad).expect_err("must reject");
            assert!(err.to_string().contains("bad_timeout"), "{err}");
        }
    }

    #[test]
    fn every_default_is_a_valid_bound() {
        for default in JobTimeout::DEFAULTS {
            assert!(default.validate().is_ok(), "{default:?} must validate");
        }
        assert!(JobTimeout::CRATE.minutes() > JobTimeout::PLAN.minutes());
        assert!(JobTimeout::CRATE.minutes() > JobTimeout::VALIDATOR.minutes());
    }

    #[test]
    fn deserialized_zero_still_fails_the_gate() {
        let timeout: JobTimeout = serde_json::from_str("0").expect("fixture holds");
        let err = timeout.validate().expect_err("serde bypass must not hold");
        assert!(err.to_string().contains("bad_timeout"), "{err}");
        let timeout: JobTimeout = serde_json::from_str("30").expect("fixture holds");
        assert!(timeout.validate().is_ok());
    }

    /// Minimal workflow JSON with a swappable job timeout.
    fn workflow_json(timeout: &str) -> String {
        format!(
            r#"{{"name":"CI","triggers":{{"pull_request_types":[],"push_branches":["main"],"merge_group":false}},"permissions":{{"contents":"read","pull_requests":"none","id_token":"none","actions":"read"}},"concurrency":{{"group":"g","cancel_in_progress":"c"}},"jobs":{{"plan":{{"display_name":"Plan","runs_on":"ubuntu-26.04","timeout_minutes":{timeout},"steps":[{{"name":"s","kind":"shell","run":["true"]}}]}}}}}}"#
        )
    }

    #[test]
    fn workflow_gate_rejects_deserialized_zero_and_absurd_timeouts() {
        use crate::workflow::ir::WorkflowIr;
        for bad in ["0", "361", "3600"] {
            let workflow: WorkflowIr =
                serde_json::from_str(&workflow_json(bad)).expect("fixture holds");
            let err = workflow.validate().expect_err("must reject");
            assert!(err.to_string().contains("bad_timeout"), "{err}");
        }
        let workflow: WorkflowIr =
            serde_json::from_str(&workflow_json("10")).expect("fixture holds");
        assert!(workflow.validate().is_ok());
    }
}
