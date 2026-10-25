//! These tests check the DDL and current-schema validation for physical attempts.

use crate::error::HostError;
use crate::journal::request_lifecycle::tests::native_pre_challenge_fixture;

#[tokio::test]
async fn physical_attempt_schema_rejects_missing_guard() -> Result<(), HostError> {
    let fixture = native_pre_challenge_fixture().await?;
    let connection = fixture.journal.connection().await?;

    super::physical_attempts::bootstrap(&connection)
        .await
        .expect("The new attempt DDL must apply.");
    super::physical_attempts::validate_v35(&connection)
        .await
        .expect("The new attempt DDL must match the required schema.");
    connection
        .execute("DROP TRIGGER physical_attempt_current_insert_guard", ())
        .await
        .map_err(|_| HostError::Journal)?;

    assert!(
        super::physical_attempts::validate_v35(&connection)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn current_schema_rejects_missing_pre_jit_network_release_guard() -> Result<(), HostError> {
    let fixture = native_pre_challenge_fixture().await?;
    let connection = fixture.journal.connection().await?;

    super::storage_network::validate_v33(&connection)
        .await
        .expect("The current network guard must match its exact V33 definition.");
    connection
        .execute("DROP TRIGGER reservation_network_transition", ())
        .await
        .map_err(|_| HostError::Journal)?;

    assert!(
        super::storage_network::validate_v33(&connection)
            .await
            .is_err()
    );
    Ok(())
}
