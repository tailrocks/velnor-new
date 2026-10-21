//! `private-revoke-v1`: delete the installation token and require the API to
//! reject it afterwards (HD-7).

use std::path::PathBuf;
use std::time::Duration;

use crate::OrchestratorError;
use crate::config::private_fetch_session::{Token, http_status, scrub_dir};
use crate::internal::internal;

/// Status transport for the revoke; the real one is `curl` under the fetch policy.
pub(crate) trait StatusBackend {
    fn status(&self, method: &str, route: &str) -> Result<u16, OrchestratorError>;
    /// Wait between probes; a fake records the wait instead of sleeping.
    fn pause(&self, duration: Duration);
}

/// Waits between the post-revoke probes. GitHub invalidates a deleted
/// installation token eventually, so the first probe can still authenticate;
/// the budget is bounded (29.5 s in total) and the revoke fails closed when
/// the token still authenticates at the end.
const PROBE_BACKOFF_MS: [u64; 7] = [500, 1_000, 2_000, 4_000, 8_000, 8_000, 6_000];

struct CurlStatus {
    runner_temp: PathBuf,
    token: Token,
}

impl StatusBackend for CurlStatus {
    fn status(&self, method: &str, route: &str) -> Result<u16, OrchestratorError> {
        http_status(&self.runner_temp, &self.token, method, route)
    }

    fn pause(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Delete the installation token, then require the API to reject it.
///
/// A token that is already dead (401) counts as revoked. Token invalidation
/// is eventually consistent, so any other probe answer is retried with a
/// bounded backoff.
///
/// # Errors
/// Fails closed when the delete is refused or the token still authenticates
/// once the backoff budget is spent.
pub(crate) fn revoke(backend: &dyn StatusBackend) -> Result<(), OrchestratorError> {
    match backend.status("DELETE", "installation/token")? {
        204 | 401 => {}
        other => return Err(internal(&format!("private_token_revoke_refused:{other}"))),
    }
    let mut waits = PROBE_BACKOFF_MS.iter();
    loop {
        match backend.status("GET", "installation/repositories")? {
            401 => return Ok(()),
            other => match waits.next() {
                Some(millis) => backend.pause(Duration::from_millis(*millis)),
                None => return Err(internal(&format!("private_token_still_valid:{other}"))),
            },
        }
    }
}

/// Run `private-revoke-v1` from the step environment.
///
/// # Errors
/// See `revoke`; also removes any leftover credential directory.
pub fn run_private_revoke() -> Result<(), OrchestratorError> {
    let token = Token::new(
        &std::env::var(velnor_actions_contract::PRIVATE_FETCH_TOKEN_ENV).unwrap_or_default(),
    )?;
    let temp = std::env::var_os("RUNNER_TEMP")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| internal("private_revoke_missing_runner_temp"))?;
    scrub_dir(&temp.join("velnor").join("private-fetch"))?;
    revoke(&CurlStatus {
        runner_temp: temp,
        token,
    })?;
    println!("velnor: private-source token revoked");
    Ok(())
}

#[cfg(test)]
#[path = "private_revoke_tests.rs"]
mod tests;
