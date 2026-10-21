//! Test shutdown and retries with the journal for one repository connection.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::{
    CLAIM_LEASE_SECONDS, Context, EffectBudget, Failure, cleanup_retry_delay_seconds,
    completion_now, reconcile_due_burst, run_effect,
};
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::request_lifecycle::tests::{
    NativeStartedIntakeFixture, native_started_intake_fixture,
};
use crate::launch::completion::diagnostics::DiagnosticsRoot;
use crate::launch::completion::journal_ref::CompletionJournal;
use crate::listen::Secret;

async fn completed_fixture() -> Result<NativeStartedIntakeFixture, HostError> {
    let fixture = native_started_intake_fixture().await?;
    assert_eq!(
        fixture
            .scope
            .record_job_completed(
                &fixture.assignment,
                fixture.message_id,
                &fixture.source,
                Some("success"),
                Some(&fixture.runner),
                Some("shutdown-test-job"),
            )
            .await?,
        Some(fixture.intent_id)
    );
    Ok(fixture)
}

#[test]
fn scoped_shutdown_does_not_claim_due_work() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "scoped_shutdown_does_not_claim_due_work",
        || Box::pin(scoped_shutdown_does_not_claim_due_work_inner()),
    )
}

async fn scoped_shutdown_does_not_claim_due_work_inner() -> Result<(), String> {
    let fixture = completed_fixture()
        .await
        .map_err(|error| error.to_string())?;
    let completion = CompletionJournal::scoped(fixture.scope.clone());
    let now = completion_now().map_err(|error| error.to_string())?;
    let due = fixture
        .scope
        .due_completed_launches(now, 4)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(due.len(), 1);

    let state = fixture
        .database_path
        .parent()
        .ok_or_else(|| "The test fixture has no state directory.".to_owned())?;
    let state = std::fs::canonicalize(state).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let diagnostics = DiagnosticsRoot::open(&state, fixture.scope.connection_id().as_str())
        .map_err(|error| error.to_string())?;
    let socket = SocketFile::new()?;
    let socket_text = socket
        .0
        .to_str()
        .ok_or_else(|| "The socket path is not valid UTF-8.".to_owned())?;
    let docker = bollard::Docker::connect_with_unix(socket_text, 120, bollard::API_DEFAULT_VERSION)
        .map_err(|error| error.to_string())?;
    let mut transport =
        HttpsTransport::new("https://github.com").map_err(|error| error.to_string())?;
    let admin = Secret::new("test token");
    let context = Context {
        journal: &completion,
        docker: &docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: None,
        selected_engine: None,
    };
    let mut context = context;
    let stopping = AtomicBool::new(true);
    let (processed, failure) = reconcile_due_burst(&mut context, &stopping)
        .await
        .map_err(|failure| failure.error.to_string())?;

    assert_eq!(processed, 0);
    assert_eq!(failure, None);
    assert_eq!(
        fixture
            .scope
            .due_completed_launches(now, 4)
            .await
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn stop_after_scoped_claim_schedules_retry_without_effect() -> Result<(), HostError> {
    let fixture = completed_fixture().await?;
    let now = completion_now()?;
    let id = fixture.intent_id;
    let claim = fixture
        .scope
        .claim_completion_cleanup_at(id, now, CLAIM_LEASE_SECONDS)
        .await?
        .ok_or(HostError::Journal)?;
    let completion = CompletionJournal::scoped(fixture.scope.clone());
    let stopping = AtomicBool::new(true);
    let mut budget = EffectBudget::new(&stopping);
    let effects = AtomicUsize::new(0);
    let result = run_effect(
        &completion,
        &mut budget,
        id,
        claim,
        "scoped test request",
        || async {
            effects.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    )
    .await;
    let failure = result.err().ok_or(HostError::Journal)?;
    assert_eq!(failure, Failure::stopped("scoped test request"));
    assert_eq!(effects.load(Ordering::Relaxed), 0);
    let delay = cleanup_retry_delay_seconds(claim.attempt);
    let retry_at = now.checked_add(delay).ok_or(HostError::Journal)?;
    assert!(
        fixture
            .scope
            .retry_completion_cleanup_at(id, claim.generation, now, retry_at)
            .await?
    );
    let due_before_retry = fixture
        .scope
        .due_completed_launches(retry_at - 1, 4)
        .await?
        .into_iter()
        .next()
        .map(|launch| launch.intent.id);
    assert_eq!(due_before_retry, None);
    assert!(
        fixture
            .scope
            .claim_completion_cleanup_at(id, retry_at, CLAIM_LEASE_SECONDS)
            .await?
            .is_some()
    );
    Ok(())
}

#[tokio::test]
async fn scoped_request_failure_remains_visible_after_retry_is_scheduled() -> Result<(), HostError>
{
    let fixture = completed_fixture().await?;
    let now = completion_now()?;
    let id = fixture.intent_id;
    let claim = fixture
        .scope
        .claim_completion_cleanup_at(id, now, CLAIM_LEASE_SECONDS)
        .await?
        .ok_or(HostError::Journal)?;
    let completion = CompletionJournal::scoped(fixture.scope.clone());
    let stopping = AtomicBool::new(false);
    let mut budget = EffectBudget::new(&stopping);
    let failure = run_effect(
        &completion,
        &mut budget,
        id,
        claim,
        "container delete",
        || async { Err::<(), _>(HostError::Docker) },
    )
    .await
    .err()
    .ok_or(HostError::Journal)?;
    assert_eq!(
        failure,
        Failure::request("container delete", HostError::Docker)
    );
    let delay = cleanup_retry_delay_seconds(claim.attempt);
    let retry_at = now.checked_add(delay).ok_or(HostError::Journal)?;
    assert!(
        fixture
            .scope
            .retry_completion_cleanup_at(id, claim.generation, now, retry_at)
            .await?
    );
    assert_eq!(failure.reason, "docker");
    let due_before_retry = fixture
        .scope
        .due_completed_launches(retry_at - 1, 4)
        .await?
        .into_iter()
        .next()
        .map(|launch| launch.intent.id);
    assert_eq!(due_before_retry, None);
    assert!(
        fixture
            .scope
            .claim_completion_cleanup_at(id, retry_at, CLAIM_LEASE_SECONDS)
            .await?
            .is_some()
    );
    Ok(())
}

#[path = "scoped_shutdown_tests/worker.rs"]
mod worker;

#[path = "scoped_shutdown_tests/burst.rs"]
mod burst;

struct SocketFile(PathBuf, std::os::unix::net::UnixListener);

impl SocketFile {
    fn new() -> Result<Self, String> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-scoped-shutdown-{}-{id}.sock",
            std::process::id()
        ));
        let listener =
            std::os::unix::net::UnixListener::bind(&path).map_err(|error| error.to_string())?;
        Ok(Self(path, listener))
    }
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        if self.1.local_addr().is_err() {
            return;
        }
        if let Err(error) = std::fs::remove_file(&self.0) {
            let _kind = error.kind();
        }
    }
}
