//! Post-delete settlement holds without remote proof.

use crate::HostError;

use super::super::remove_worker_volumes;
use super::{DockerStub, WORKER, http, volume_json};

#[tokio::test]
async fn post_delete_non_not_found_returns_docker_error() -> Result<(), String> {
    let stub = DockerStub::open(vec![
        http(200, &volume_json("wtransport", WORKER, "socket")),
        http(204, ""),
        http(500, r#"{"message":"not absent"}"#),
    ])?;
    let removed = remove_worker_volumes(&stub.docker, WORKER).await;
    let requests = stub.finish().await?;

    assert_eq!(removed, Err(HostError::Docker));
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET "));
    assert!(requests[1].starts_with("DELETE "));
    assert!(requests[2].starts_with("GET "));
    Ok(())
}
