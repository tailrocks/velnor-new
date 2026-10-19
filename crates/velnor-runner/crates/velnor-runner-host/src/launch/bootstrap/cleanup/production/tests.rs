use crate::launch::inspect_tests::{DockerStub, http};
use crate::{
    error::HostError,
    launch::bootstrap::cleanup::production::{require_container_absent, require_volume_absent},
};

#[tokio::test]
async fn container_absence_requires_exact_not_found() -> Result<(), String> {
    let absent = DockerStub::open(vec![http(404, r#"{"message":"missing"}"#)])?;
    assert_eq!(
        require_container_absent(&absent.docker, "immutable-id").await,
        Ok(())
    );
    let requests = absent.finish().await?;
    if !requests[0].contains("/containers/immutable-id/json") {
        return Err("Container lookup did not use its immutable ID.".to_owned());
    }

    let present = DockerStub::open(vec![http(200, r#"{"Id":"foreign-id"}"#)])?;
    assert_eq!(
        require_container_absent(&present.docker, "expected-id").await,
        Err(HostError::Ownership)
    );
    let _requests = present.finish().await?;
    Ok(())
}

#[tokio::test]
async fn container_transport_errors_do_not_prove_absence() -> Result<(), String> {
    let failed = DockerStub::open(vec![http(500, r#"{"message":"unavailable"}"#)])?;
    assert_eq!(
        require_container_absent(&failed.docker, "immutable-id").await,
        Err(HostError::Docker)
    );
    let _requests = failed.finish().await?;
    Ok(())
}

#[tokio::test]
async fn volume_absence_requires_exact_not_found() -> Result<(), String> {
    let absent = DockerStub::open(vec![http(404, r#"{"message":"missing"}"#)])?;
    assert_eq!(
        require_volume_absent(&absent.docker, "owned-volume").await,
        Ok(())
    );
    let requests = absent.finish().await?;
    if !requests[0].contains("/volumes/owned-volume") {
        return Err("Volume lookup did not use its recorded name.".to_owned());
    }

    let present = DockerStub::open(vec![http(
        200,
        r#"{"Name":"foreign-volume","Driver":"local","Mountpoint":"/var/lib/docker/volumes/foreign-volume/_data","Labels":{},"Options":{},"Scope":"local"}"#,
    )])?;
    assert_eq!(
        require_volume_absent(&present.docker, "owned-volume").await,
        Err(HostError::Ownership)
    );
    let _requests = present.finish().await?;
    Ok(())
}
