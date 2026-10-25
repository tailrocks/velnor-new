use std::{ffi::OsString, path::Path, time::Instant};

use velnor_actions_contract::workflow::JobTimeout;
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

const TIMEOUT_ENV: &str = "VELNOR_RELEASE_PUBLICATION_TIMEOUT_MINUTES";

pub(super) fn with_env<T>(
    operation: impl FnOnce(CheckDeadline) -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    let started = Instant::now();
    with_config(std::env::var_os(TIMEOUT_ENV), started, operation)
}

fn with_config<T>(
    raw: Option<OsString>,
    started: Instant,
    operation: impl FnOnce(CheckDeadline) -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    let timeout = parse_timeout(raw)?;
    let budget = std::time::Duration::from_secs(u64::from(timeout.minutes()) * 60);
    let deadline = CheckDeadline::from_start_with_cleanup_reserve(started, budget)
        .map_err(|_| internal("package_publication_deadline_invalid"))?;
    with_deadline(deadline, operation)
}

pub(super) fn with_deadline<T>(
    deadline: CheckDeadline,
    operation: impl FnOnce(CheckDeadline) -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    deadline
        .scope(|| operation(deadline))
        .map_err(|_| internal("package_publication_deadline"))?
}

pub(crate) fn with_default_scope<T>(
    operation: impl FnOnce(CheckDeadline) -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    let budget = std::time::Duration::from_secs(u64::from(JobTimeout::DEFAULT_MINUTES) * 60);
    let deadline = CheckDeadline::from_start_with_cleanup_reserve(Instant::now(), budget)
        .map_err(|_| internal("package_publication_subscope_deadline"))?;
    deadline
        .scope(|| operation(deadline))
        .map_err(|_| internal("package_publication_subscope_deadline"))?
}

pub(crate) fn hold_signed_directory<T>(
    directory: &Path,
    custody_deadline: CheckDeadline,
    verify: impl FnOnce() -> Result<T, OrchestratorError>,
) -> Result<(T, crate::filesystem_owner::held_directory::HeldDirectory), OrchestratorError> {
    crate::filesystem_owner::held_directory::HeldDirectory::admit(
        directory,
        custody_deadline,
        verify,
    )
}

fn parse_timeout(raw: Option<OsString>) -> Result<JobTimeout, OrchestratorError> {
    let Some(raw) = raw else {
        return Ok(JobTimeout::DEFAULT);
    };
    let value = raw
        .into_string()
        .map_err(|_| internal("package_publication_timeout_invalid"))?;
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || (bytes.len() > 1 && bytes[0] == b'0')
        || !bytes.iter().all(u8::is_ascii_digit)
    {
        return Err(internal("package_publication_timeout_invalid"));
    }
    let minutes = value
        .parse::<u16>()
        .map_err(|_| internal("package_publication_timeout_invalid"))?;
    JobTimeout::new(minutes).map_err(|_| internal("package_publication_timeout_invalid"))
}
