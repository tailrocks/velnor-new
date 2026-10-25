use crate::{OrchestratorError, internal::internal};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use velnor_actions_mise::CheckDeadline;

pub(super) fn absolute_deadline() -> Result<CheckDeadline, OrchestratorError> {
    let epoch: u64 = std::env::var("VELNOR_HOME_BUDGET_DEADLINE_EPOCH")
        .map_err(|_| internal("homebrew_qualification_budget_missing"))?
        .parse()
        .map_err(|_| internal("homebrew_qualification_budget_invalid"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| internal("homebrew_qualification_clock"))?
        .as_secs();
    let remaining = epoch
        .checked_sub(now)
        .filter(|s| *s > 0)
        .ok_or_else(|| internal("homebrew_qualification_budget_expired"))?;
    CheckDeadline::after(Duration::from_secs(remaining))
        .map_err(|_| internal("homebrew_qualification_deadline"))
}
