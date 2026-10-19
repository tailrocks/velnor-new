use super::{DockerStub, WORKER, http, volume_json, volume_names};
use crate::HostError;
use crate::worker::{create_named_volumes, dind_mounts};

#[tokio::test]
async fn existing_owned_volume_prevents_all_create_requests() -> Result<(), String> {
    for (index, (name, role)) in volume_names().iter().enumerate() {
        let mut responses: Vec<_> = (0..index)
            .map(|_| http(404, r#"{"message":"missing"}"#))
            .collect();
        responses.push(http(200, &volume_json(name, WORKER, role)));
        let stub = DockerStub::open(responses)?;
        let mounts = dind_mounts(WORKER).map_err(|error| error.to_string())?;
        let result = create_named_volumes(&stub.docker, WORKER, &mounts, || async { Ok(()) }).await;
        let requests = stub.finish().await?;
        assert_eq!(result, Err(HostError::Ownership));
        assert_eq!(requests.len(), index + 1);
        assert!(requests.iter().all(|request| request.starts_with("GET ")));
    }
    Ok(())
}

#[tokio::test]
async fn uncertain_volume_inspect_prevents_all_create_requests() -> Result<(), String> {
    for status in [401, 403, 500, 503] {
        let stub = DockerStub::open(vec![http(status, r#"{"message":"unavailable"}"#)])?;
        let mounts = dind_mounts(WORKER).map_err(|error| error.to_string())?;
        let result = create_named_volumes(&stub.docker, WORKER, &mounts, || async { Ok(()) }).await;
        let requests = stub.finish().await?;
        assert_eq!(result, Err(HostError::Docker));
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET "));
    }
    Ok(())
}

#[tokio::test]
async fn failed_freshness_check_stops_the_next_volume_create() -> Result<(), String> {
    for failed_check in 0..volume_names().len() {
        let names = volume_names();
        let mut responses: Vec<_> = names
            .iter()
            .map(|_| http(404, r#"{"message":"missing"}"#))
            .collect();
        responses.extend(
            names
                .iter()
                .take(failed_check)
                .map(|(name, role)| http(201, &volume_json(name, WORKER, role))),
        );
        let stub = DockerStub::open(responses)?;
        let mounts = dind_mounts(WORKER).map_err(|error| error.to_string())?;
        let mut checks = 0;
        let result = create_named_volumes(&stub.docker, WORKER, &mounts, || {
            let check = checks;
            checks += 1;
            async move {
                if check == failed_check {
                    Err(HostError::Ownership)
                } else {
                    Ok(())
                }
            }
        })
        .await;
        let requests = stub.finish().await?;

        assert_eq!(result, Err(HostError::Ownership));
        assert_eq!(checks, failed_check + 1);
        assert_eq!(requests.len(), names.len() + failed_check);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.starts_with("POST "))
                .count(),
            failed_check
        );
    }
    Ok(())
}
