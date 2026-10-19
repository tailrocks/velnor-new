use super::drive::GuestAdmission;
use super::{Rest, steps};
use crate::launch_harness::{available, open};
use crate::scale_set::EnsureError;
use crate::worker::{guest_image::test_support::TestGuestImage, test_resource_budget};

#[tokio::test]
async fn incomplete_source_is_rejected_by_production_pre_acquire_gate() -> Result<(), String> {
    let (_scratch, journal) = open("source-before-acquire").await?;
    let fixture = TestGuestImage::new().map_err(|error| error.to_string())?;
    let connection = &fixture.connection;
    let scope = journal
        .connection_scope(connection)
        .await
        .map_err(|error| error.to_string())?;
    let polled = available(&[4]);
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let rest = Rest {
        owner: "acme",
        repo: "project",
        pat: "pat",
        resource_budget: Some(budget),
        static_capacity: true,
        guest_admission: GuestAdmission::FreshSample,
        connection: Some(connection),
        authority_policy: None,
        host_journal: Some(&journal),
        release_read_secret: None,
        scoped_journal: Some(&scope),
        allocated_header: 1,
        source_authorized: false,
        admission_state: None,
        worker_images: Some(&fixture.images),
        selected_engine: Some(&fixture.selected_engine),
        guest_image: Some(&fixture.file),
    };

    assert_eq!(
        steps::authorize_poll_rest(&rest, &polled).await,
        Err(EnsureError::SourceBlocked {
            reason: "source_metadata_missing",
        })
    );
    assert!(
        scope
            .physical_runner_reservations()
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn scoped_admission_rejects_missing_source_before_probe_or_acquire() -> Result<(), String> {
    use crate::{
        config::ApprovedJobAuthorityPolicy,
        launch::{ScopedAdmissionInput, acquire_scoped_offer},
        listen::inert_link_for_offline_test,
        worker::test_resource_budget,
    };
    use bollard::Docker;

    let (_scratch, journal) = open("source-order-scoped-admission").await?;
    let fixture = TestGuestImage::new().map_err(|error| error.to_string())?;
    let scope = journal
        .connection_scope(&fixture.connection)
        .await
        .map_err(|error| error.to_string())?;
    let connection_id = serde_json::to_value(&fixture.connection.connection_id)
        .map_err(|error| error.to_string())?;
    let policy: ApprovedJobAuthorityPolicy = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "connection_id": connection_id,
        "binding_sha256": "a".repeat(64),
        "generation": 1,
        "policy_sha256": "b".repeat(64),
        "generator": {
            "source_sha": "c".repeat(40),
            "artifact_sha256": "d".repeat(64)
        },
        "workflows": []
    }))
    .map_err(|error| error.to_string())?;
    let mut link = inert_link_for_offline_test();
    let admin_token = link.token().to_owned();
    let socket_path = std::path::PathBuf::from(format!(
        "/tmp/v{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    let _listener =
        tokio::net::UnixListener::bind(&socket_path).map_err(|error| error.to_string())?;
    let socket = socket_path
        .to_str()
        .ok_or_else(|| "Docker socket path is not UTF-8".to_owned())?;
    let docker = Docker::connect_with_socket(socket, 1, &bollard::API_DEFAULT_VERSION)
        .map_err(|error| error.to_string())?;
    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let secret = crate::launch::resource_probe::ReleaseReadSecret::test_inert();
    let admission_state = std::env::temp_dir();
    let polled = available(&[4]);
    let result = acquire_scoped_offer(ScopedAdmissionInput {
        link: &mut link,
        queue_origin: None,
        queue_path: "/messages".to_owned(),
        queue_token: "test-queue-token".to_owned(),
        admin_token,
        set_id: 501,
        polled: &polled,
        docker: &docker,
        connection: &fixture.connection,
        authority_policy: &policy,
        pat: "test-pat",
        host_journal: &journal,
        scoped_journal: &scope,
        release_read_secret: &secret,
        selected_engine: &fixture.selected_engine,
        worker_images: &fixture.images,
        guest_image: &fixture.file,
        resource_budget: budget,
        allocated_header: 1,
        admission_state: &admission_state,
    })
    .await;

    assert!(matches!(
        result,
        Err(EnsureError::SourceBlocked {
            reason: "source_metadata_missing"
        })
    ));
    assert!(
        scope
            .physical_runner_reservations()
            .await
            .map_err(|error| error.to_string())?
            .is_empty()
    );
    drop(_listener);
    std::fs::remove_file(socket_path).map_err(|error| error.to_string())?;
    Ok(())
}
