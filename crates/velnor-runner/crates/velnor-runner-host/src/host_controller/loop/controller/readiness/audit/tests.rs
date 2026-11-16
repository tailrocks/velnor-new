// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::{ExpectedContainer, OwnedPair, audit_pair_containers};
use crate::error::HostError;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const RUNNER_ID: &str = "a000000000000000000000000000000000000000000000000000000000000004";
const DIND_ID: &str = "d000000000000000000000000000000000000000000000000000000000000004";
const RUNNER_IMAGE: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DIND_IMAGE: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const OTHER_IMAGE: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const OWNER: OwnedPair<'static> = OwnedPair {
    connection: "lab",
    reservation: 4,
    operation: 3,
};

fn container(role: &str, image: &str, kind: Option<&str>) -> Value {
    let mut labels = json!({
        "io.velnor.managed": "true",
        "io.velnor.reservation-id": "4",
        "io.velnor.operation-id": "3",
        "io.velnor.connection-id": "lab",
        "io.velnor.role": role,
    });
    if let Some(kind) = kind {
        labels["io.velnor.image.kind"] = json!(kind);
    }
    let id = if role == "runner" { RUNNER_ID } else { DIND_ID };
    json!({"Id": id, "Name": format!("/w4-{role}"), "Image": image, "State": {"Running": true}, "Config": {"Labels": labels}})
}

fn healthy() -> BTreeMap<String, Value> {
    BTreeMap::from([
        (
            RUNNER_ID.to_owned(),
            container("runner", RUNNER_IMAGE, Some("runner")),
        ),
        (
            DIND_ID.to_owned(),
            container("dind", DIND_IMAGE, Some("dind")),
        ),
    ])
}

fn expected() -> [ExpectedContainer<'static>; 2] {
    [
        ExpectedContainer {
            id: RUNNER_ID,
            name: "w4-runner".to_owned(),
            role: "runner",
            image_id: RUNNER_IMAGE,
        },
        ExpectedContainer {
            id: DIND_ID,
            name: "w4-dind".to_owned(),
            role: "dind",
            image_id: DIND_IMAGE,
        },
    ]
}

async fn audit(containers: BTreeMap<String, Value>) -> TestResult<Result<(), HostError>> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    serve(&socket, containers)?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    Ok(audit_pair_containers(&docker, &OWNER, &expected()).await)
}

fn serve(socket: &Path, containers: BTreeMap<String, Value>) -> TestResult<()> {
    let listener = UnixListener::bind(socket)?;
    let containers = Arc::new(containers);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let containers = Arc::clone(&containers);
            tokio::spawn(async move { drop(respond(stream, &containers).await) });
        }
    });
    Ok(())
}

async fn respond(mut stream: UnixStream, containers: &BTreeMap<String, Value>) -> TestResult<()> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err("the request closed early".into());
        }
        request.extend_from_slice(&chunk[..read]);
    }
    let head = String::from_utf8_lossy(&request).into_owned();
    let target = head.split_whitespace().nth(1).unwrap_or_default();
    let segments: Vec<&str> = target.split('/').skip(1).collect();
    let found = match segments.as_slice() {
        ["containers", id, "json"] => containers.get(*id),
        _ => None,
    };
    let (status, body) = found.map_or((404, json!({"message": "No such container"})), |body| {
        (200, body.clone())
    });
    let body = body.to_string();
    let reply = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(reply.as_bytes()).await?;
    Ok(stream.shutdown().await?)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_with_matching_kind_labels_and_admitted_images_is_owned() -> TestResult<()> {
    assert_eq!(audit(healthy()).await?, Ok(()));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_whose_container_lacks_the_image_kind_label_is_refused() -> TestResult<()> {
    for id in [RUNNER_ID, DIND_ID] {
        let mut containers = healthy();
        if let Some(labels) = containers
            .get_mut(id)
            .and_then(|value| value["Config"]["Labels"].as_object_mut())
        {
            labels.remove("io.velnor.image.kind");
        }
        assert_eq!(audit(containers).await?, Err(HostError::Ownership), "{id}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_whose_container_carries_the_other_roles_image_kind_is_refused() -> TestResult<()> {
    let mut containers = healthy();
    containers.insert(
        RUNNER_ID.to_owned(),
        container("runner", RUNNER_IMAGE, Some("dind")),
    );
    assert_eq!(audit(containers).await?, Err(HostError::Ownership));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_with_a_container_from_another_image_is_refused() -> TestResult<()> {
    for (role, id) in [("runner", RUNNER_ID), ("dind", DIND_ID)] {
        let mut containers = healthy();
        containers.insert(id.to_owned(), container(role, OTHER_IMAGE, Some(role)));
        assert_eq!(
            audit(containers).await?,
            Err(HostError::Ownership),
            "{role}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_with_a_stopped_or_foreign_reservation_container_is_refused() -> TestResult<()> {
    let mut stopped = healthy();
    if let Some(value) = stopped.get_mut(DIND_ID) {
        value["State"]["Running"] = json!(false);
    }
    assert_eq!(audit(stopped).await?, Err(HostError::Ownership));
    let mut foreign = healthy();
    if let Some(value) = foreign.get_mut(RUNNER_ID) {
        value["Config"]["Labels"]["io.velnor.reservation-id"] = json!("9");
    }
    assert_eq!(audit(foreign).await?, Err(HostError::Ownership));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pair_with_an_unknown_velnor_label_or_missing_container_is_refused() -> TestResult<()> {
    let mut extra = healthy();
    if let Some(value) = extra.get_mut(RUNNER_ID) {
        value["Config"]["Labels"]["io.velnor.extra"] = json!("1");
    }
    assert_eq!(audit(extra).await?, Err(HostError::Ownership));
    let mut missing = healthy();
    missing.remove(DIND_ID);
    assert_eq!(audit(missing).await?, Err(HostError::Docker));
    Ok(())
}
