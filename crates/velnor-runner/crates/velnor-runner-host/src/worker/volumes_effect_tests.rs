//! The opaque verified-volume token exposes one Docker request per method.

use super::{DockerStub, WORKER, http, volume_json};
use crate::worker::{WorkerVolumeRole, WorkerVolumeVerification, verify_worker_volume};

#[tokio::test]
async fn verified_delete_and_absence_confirmation_are_separate_requests() -> Result<(), String> {
    let stub = DockerStub::open(vec![
        http(200, &volume_json("wtransport", WORKER, "socket")),
        http(204, ""),
        http(404, r#"{"message":"missing"}"#),
    ])?;
    let verified = match verify_worker_volume(&stub.docker, WORKER, WorkerVolumeRole::Socket).await
    {
        Ok(WorkerVolumeVerification::Verified(verified)) => verified,
        Ok(WorkerVolumeVerification::Absent | WorkerVolumeVerification::OwnershipMismatch) => {
            return Err("verified volume token was not returned".to_owned());
        }
        Err(error) => return Err(error.to_string()),
    };
    verified
        .remove_request(&stub.docker)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        verified
            .confirm_removed(&stub.docker)
            .await
            .map_err(|error| error.to_string())?,
        crate::worker::WorkerVolumeRemoval::Removed
    );
    let requests = stub.finish().await?;
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET "));
    assert!(requests[1].starts_with("DELETE "));
    assert!(requests[2].starts_with("GET "));
    Ok(())
}
