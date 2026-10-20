use std::time::Duration;

use crate::journal::{BootstrapContainerCreateClaim, ConnectionJournal};

const JOURNAL_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn record_terminal_create_response(
    journal: &ConnectionJournal,
    claim: &BootstrapContainerCreateClaim,
    error: bollard::errors::Error,
) {
    let bollard::errors::Error::DockerResponseServerError { status_code, .. } = error else {
        return;
    };
    if !terminal_create_response_status(status_code) {
        return;
    }
    let api = bollard::API_DEFAULT_VERSION;
    let (Ok(api_major), Ok(api_minor)) = (
        u16::try_from(api.major_version),
        u16::try_from(api.minor_version),
    ) else {
        return;
    };
    let _recorded = tokio::time::timeout(
        JOURNAL_TIMEOUT,
        journal.record_bootstrap_container_create_terminal_response(
            claim,
            status_code,
            api_major,
            api_minor,
        ),
    )
    .await;
}

fn terminal_create_response_status(status_code: u16) -> bool {
    matches!(status_code, 400 | 404 | 409)
}

#[cfg(test)]
#[path = "terminal_response/tests.rs"]
mod tests;
