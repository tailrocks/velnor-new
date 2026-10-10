use std::time::Duration;

use bollard::ClientVersion;
use tokio::time::Instant;

use super::*;

#[tokio::test]
async fn rejects_engine_or_docker_root_drift_after_image_inspection() -> Result<(), HostError> {
    for (engine, root) in [
        ("different-engine", DOCKER_ROOT),
        (ENGINE_ID, "/other/root"),
    ] {
        let (result, requests) = run_existing_image(
            containerd_image()?,
            PostInspectInfo::Changed(info_response(engine, root)),
        )
        .await?;
        assert!(matches!(result, Err(HostError::Identity)));
        assert_eq!(requests.len(), 3, "{requests:?}");
        assert_inspect_query(&requests[1]);
        assert_no_import(&requests);
    }
    Ok(())
}

#[tokio::test]
async fn rejects_api_below_platform_support_without_inspecting_or_importing()
-> Result<(), HostError> {
    let daemon = TestDaemon::open(
        vec![Reply {
            status: 200,
            body: info_response(ENGINE_ID, DOCKER_ROOT),
        }],
        ClientVersion {
            major_version: 1,
            minor_version: 48,
        },
    )?;
    let result = load_and_inspect(
        &daemon.docker,
        ENGINE_ID,
        DOCKER_ROOT,
        &recorded_release()?,
        Instant::now() + Duration::from_secs(3),
    )
    .await;
    let requests = daemon.finish().await?;

    assert!(matches!(result, Err(HostError::Identity)));
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET /info "), "{}", requests[0]);
    assert_no_import(&requests);
    Ok(())
}
