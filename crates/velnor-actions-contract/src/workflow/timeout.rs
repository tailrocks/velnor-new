//! Bounded literal timeouts for every executable generated job.
//!
//! Every built-in job defaults to five minutes. Configuration may select any
//! whole-minute budget up to GitHub's 360-minute job execution limit; nothing
//! is clamped. The required typed field and workflow validation also reject
//! serde bypasses.

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
    /// Largest configurable bound: GitHub's per-job execution limit (six hours).
    pub const MAX_MINUTES: u16 = 360;
    /// Default for every built-in job and every omitted configured timeout.
    pub const DEFAULT_MINUTES: u16 = 5;
    /// All built-in jobs use five minutes unless configuration selects another bound.
    pub const DEFAULT: Self = Self(Self::DEFAULT_MINUTES);

    /// Planner job.
    pub const PLAN: Self = Self::DEFAULT;
    /// Crate job default; `workflow.crate_timeout_minutes` overrides it.
    pub const CRATE: Self = Self::DEFAULT;
    /// Required gate.
    pub const REQUIRED: Self = Self::DEFAULT;
    /// Validator job.
    pub const VALIDATOR: Self = Self::DEFAULT;
    /// Candidate job.
    pub const CANDIDATE: Self = Self::DEFAULT;
    /// Release job.
    pub const RELEASE: Self = Self::DEFAULT;
    /// MSRV job.
    pub const MSRV: Self = Self::DEFAULT;
    /// Publish job.
    pub const PUBLISH: Self = Self::DEFAULT;

    /// Validate minutes without narrowing wider configuration fields.
    #[must_use]
    pub fn is_valid_minutes(minutes: u32) -> bool {
        (u32::from(Self::MIN_MINUTES)..=u32::from(Self::MAX_MINUTES)).contains(&minutes)
    }

    /// Validate a configured timeout, naming the accepted range on failure.
    /// # Errors
    pub fn check_config(minutes: u32, file: &str, key: &str) -> Result<(), ContractError> {
        if Self::is_valid_minutes(minutes) {
            return Ok(());
        }
        Err(ContractError::config(
            file,
            key,
            format!(
                "bad_timeout:{minutes}:expected_{}..={}",
                Self::MIN_MINUTES,
                Self::MAX_MINUTES
            ),
        ))
    }

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
        if !Self::is_valid_minutes(u32::from(self.0)) {
            return Err(ContractError::identity(
                "job.timeout_minutes",
                format!("bad_timeout:{}", self.0),
            ));
        }
        Ok(())
    }

    /// Whole minutes for the renderer.
    #[must_use]
    pub const fn minutes(&self) -> u16 {
        self.0
    }
}

impl TryFrom<u32> for JobTimeout {
    type Error = ContractError;

    fn try_from(minutes: u32) -> Result<Self, Self::Error> {
        let minutes = u16::try_from(minutes).map_err(|_| {
            ContractError::identity("job.timeout_minutes", format!("bad_timeout:{minutes}"))
        })?;
        Self::new(minutes)
    }
}

#[cfg(test)]
mod tests {
    use super::JobTimeout;

    #[test]
    fn range_edges_hold_and_beyond_fails() {
        for minutes in [1, 5, 6, 30, 359, 360] {
            assert_eq!(
                JobTimeout::new(minutes).expect("valid budget").minutes(),
                minutes
            );
        }
        assert!(!JobTimeout::is_valid_minutes(u32::MAX));
        for bad in [0, 361, 1000, u16::MAX] {
            let err = JobTimeout::new(bad).expect_err("must reject");
            assert!(err.to_string().contains("bad_timeout"), "{err}");
        }
    }

    #[test]
    fn wide_constructor_preserves_boundaries_and_full_invalid_value() {
        for minutes in [1_u32, 5, 360] {
            assert_eq!(
                u32::from(JobTimeout::try_from(minutes).expect("valid").minutes()),
                minutes
            );
        }
        for minutes in [0_u32, 361, u32::MAX] {
            let error = JobTimeout::try_from(minutes).expect_err("invalid budget");
            assert!(
                error
                    .to_string()
                    .contains(&format!("bad_timeout:{minutes}")),
                "{error}"
            );
        }
    }

    #[test]
    fn every_default_is_a_valid_bound() {
        for default in JobTimeout::DEFAULTS {
            assert!(default.validate().is_ok(), "{default:?} must validate");
            assert_eq!(default, JobTimeout::DEFAULT);
        }
        assert_eq!(JobTimeout::CRATE, JobTimeout::DEFAULT);
        assert_eq!(JobTimeout::DEFAULT.minutes(), 5);
        assert_eq!(JobTimeout::DEFAULT_MINUTES, 5);
        assert_eq!(JobTimeout::MAX_MINUTES, 360);
    }

    #[test]
    fn deserialized_zero_still_fails_the_gate() {
        let timeout: JobTimeout = serde_json::from_str("0").expect("fixture holds");
        let err = timeout.validate().expect_err("serde bypass must not hold");
        assert!(err.to_string().contains("bad_timeout"), "{err}");
        let timeout: JobTimeout = serde_json::from_str("5").expect("fixture holds");
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
        for good in ["1", "5", "30", "360"] {
            let workflow: WorkflowIr =
                serde_json::from_str(&workflow_json(good)).expect("fixture holds");
            assert!(workflow.validate().is_ok(), "{good}");
        }
    }
}
