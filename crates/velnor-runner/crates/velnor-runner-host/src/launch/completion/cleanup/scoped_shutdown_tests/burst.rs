//! Test completion scan limits with native records for one repository connection.

use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::AtomicBool;

use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::journal::request_lifecycle::tests::{
    NativeStartedIntakeFixture, add_twelve_completed_burst_rows,
    native_started_intake_burst_fixture,
};
use crate::launch::completion::cleanup::{
    Context, Failure, MAX_SCAN_WAVES, SCAN_LIMIT, completion_now, reconcile_due_burst,
};
use crate::launch::completion::diagnostics::DiagnosticsRoot;
use crate::launch::completion::journal_ref::CompletionJournal;
use crate::listen::Secret;

use super::SocketFile;

#[tokio::test]
async fn scoped_burst_processes_three_pages_and_leaves_the_next_row_due() -> Result<(), HostError> {
    let fixture = native_started_intake_burst_fixture().await?;
    let completed = fixture
        .scope
        .record_job_completed(
            &fixture.assignment,
            fixture.message_id,
            &fixture.source,
            Some("success"),
            Some(&fixture.runner),
            Some("shutdown-test-job"),
        )
        .await?;
    assert_eq!(completed, Some(fixture.intent_id));
    add_twelve_completed_burst_rows(&fixture).await?;
    run_scoped_burst(&fixture).await
}

async fn run_scoped_burst(fixture: &NativeStartedIntakeFixture) -> Result<(), HostError> {
    let now = completion_now()?;
    let all_due = fixture.scope.due_completed_launches(now, 32).await?;
    assert_eq!(all_due.len(), 13);
    let remaining_id = all_due.last().ok_or(HostError::Journal)?.intent.id;
    let state = fixture.database_path.parent().ok_or(HostError::Path)?;
    let state = std::fs::canonicalize(state).map_err(|_| HostError::Path)?;
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Path)?;
    let diagnostics = DiagnosticsRoot::open(&state, fixture.scope.connection_id().as_str())?;
    let socket = SocketFile::new().map_err(|_| HostError::Path)?;
    let socket_path = socket.0.to_str().ok_or(HostError::Path)?;
    let docker = bollard::Docker::connect_with_unix(socket_path, 120, bollard::API_DEFAULT_VERSION)
        .map_err(|_| HostError::Docker)?;
    let mut transport =
        HttpsTransport::new("https://github.com").map_err(|_| HostError::Journal)?;
    let admin = Secret::new("test token");
    let journal = CompletionJournal::scoped(fixture.scope.clone());
    let mut context = Context {
        journal: &journal,
        docker: &docker,
        transport: &mut transport,
        admin: &admin,
        diagnostics: &diagnostics,
        native_cleanup: None,
        selected_engine: None,
    };
    let stopping = AtomicBool::new(false);
    let (processed, failure) = reconcile_due_burst(&mut context, &stopping)
        .await
        .map_err(|failure| failure.error)?;
    let page_limit = usize::try_from(SCAN_LIMIT).map_err(|_| HostError::Journal)?;
    assert_eq!(processed, page_limit * MAX_SCAN_WAVES);
    assert_eq!(
        failure,
        Some(Failure::not_proven("native cleanup capability"))
    );
    assert_only_remaining_due_row(fixture, remaining_id).await
}

async fn assert_only_remaining_due_row(
    fixture: &NativeStartedIntakeFixture,
    expected_id: i64,
) -> Result<(), HostError> {
    let due = fixture
        .scope
        .due_completed_launches(completion_now()?, 32)
        .await?;
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].intent.id, expected_id);
    Ok(())
}
