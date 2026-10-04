//! Optional main-workflow verification cadence and dispatch settings.

use crate::errors::ContractError;
use crate::workflow::ScheduleTrigger;
use serde::{Deserialize, Serialize};

/// Optional main CI schedule and manual dispatch configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationConfig {
    /// Optional five-field cron expression for the main CI workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// Whether the main CI workflow accepts manual dispatches.
    #[serde(default)]
    pub workflow_dispatch: bool,
    /// Enable the protected failure observer and optional manual simulation.
    #[serde(default)]
    pub alert: bool,
}

impl VerificationConfig {
    /// Validate the optional schedule.
    ///
    /// # Errors
    ///
    /// Returns a config error when the schedule is not a valid five-field
    /// cron expression.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.alert && self.schedule.is_none() && !self.workflow_dispatch {
            return Err(ContractError::config(
                file,
                "workflow.verification.alert",
                "requires_schedule_or_dispatch",
            ));
        }
        let Some(schedule) = &self.schedule else {
            return Ok(());
        };
        let shape_valid = ScheduleTrigger {
            cron: vec![schedule.clone()],
        }
        .validate()
        .is_ok();
        if !shape_valid || !is_valid_cron(schedule) {
            return Err(ContractError::config(
                file,
                "workflow.verification.schedule",
                "invalid_cron",
            ));
        }
        Ok(())
    }
}

/// Validate a five-field numeric POSIX cron expression.
fn is_valid_cron(value: &str) -> bool {
    let fields: Vec<&str> = value.split(' ').collect();
    fields.len() == 5
        && fields
            .iter()
            .zip([(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)])
            .all(|(field, (min, max))| is_valid_cron_field(field, min, max))
}

/// Validate one cron field's comma, range, and step expressions.
fn is_valid_cron_field(value: &str, min: u32, max: u32) -> bool {
    value.split(',').all(|item| {
        let (range, step) = item
            .split_once('/')
            .map_or((item, None), |(range, step)| (range, Some(step)));
        if step.is_some_and(|step| !cron_number(step, 1, max - min + 1)) {
            return false;
        }
        if range == "*" {
            return true;
        }
        if let Some((start, end)) = range.split_once('-') {
            return cron_number(start, min, max)
                && cron_number(end, min, max)
                && start.parse::<u32>().ok() <= end.parse::<u32>().ok();
        }
        step.is_none() && cron_number(range, min, max)
    })
}

/// Validate one bounded decimal cron number.
fn cron_number(value: &str, min: u32, max: u32) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value
            .parse::<u32>()
            .is_ok_and(|number| (min..=max).contains(&number))
}

#[cfg(test)]
#[path = "workflow_verification_tests.rs"]
mod tests;
