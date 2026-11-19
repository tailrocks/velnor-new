// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::TestResult;
use super::seed::{CONNECTION, DIND_ID, DIND_IMAGE, ENGINE, RESERVATION, RUNNER_ID, RUNNER_IMAGE};
use crate::worker::{WorkerContainerRole, WorkerIdentityNames, WorkerVolumeRole};

pub(super) const OPERATION: i64 = RESERVATION - 1;

type Objects = (BTreeMap<String, Value>, BTreeMap<String, Value>);

pub(super) fn names() -> TestResult<WorkerIdentityNames> {
    Ok(WorkerIdentityNames::from_nonce(&super::seed::nonce())?)
}

pub(super) fn container(role: &str, image: &str, kind: Option<&str>) -> TestResult<Value> {
    let mut labels = json!({
        "io.velnor.managed": "true",
        "io.velnor.reservation-id": RESERVATION.to_string(),
        "io.velnor.operation-id": OPERATION.to_string(),
        "io.velnor.connection-id": CONNECTION,
        "io.velnor.role": role,
    });
    if let Some(kind) = kind {
        labels["io.velnor.image.kind"] = json!(kind);
    }
    let (id, role_name) = if role == "runner" {
        (RUNNER_ID, WorkerContainerRole::Runner)
    } else {
        (DIND_ID, WorkerContainerRole::Dind)
    };
    let name = names()?.container_name(role_name);
    Ok(
        json!({"Id": id, "Name": format!("/{name}"), "Image": image, "State": {"Running": true}, "Config": {"Labels": labels}}),
    )
}

pub(super) fn containers(
    runner: (&str, Option<&str>),
    dind: (&str, Option<&str>),
) -> TestResult<BTreeMap<String, Value>> {
    Ok(BTreeMap::from([
        (
            RUNNER_ID.to_owned(),
            container("runner", runner.0, runner.1)?,
        ),
        (DIND_ID.to_owned(), container("dind", dind.0, dind.1)?),
    ]))
}

pub(super) fn healthy() -> TestResult<BTreeMap<String, Value>> {
    containers((RUNNER_IMAGE, Some("runner")), (DIND_IMAGE, Some("dind")))
}

fn volumes() -> TestResult<BTreeMap<String, Value>> {
    let names = names()?;
    Ok([
        (WorkerVolumeRole::Socket, "shared-run"),
        (WorkerVolumeRole::Home, "runner-home"),
        (WorkerVolumeRole::Work, "runner-work"),
        (WorkerVolumeRole::DindData, "dind-data"),
        (WorkerVolumeRole::RunnerMetadata, "engine-metadata"),
    ]
    .into_iter()
    .map(|(role, label)| {
        let name = names.volume_name(role);
        let body = json!({"Name": name, "Driver": "local", "Mountpoint": "/m", "Scope": "local", "Options": {}, "Labels": {
            "velnor.connection": CONNECTION,
            "velnor.reservation": RESERVATION.to_string(),
            "velnor.operation": OPERATION.to_string(),
            "velnor.role": label,
        }});
        (name, body)
    })
    .collect())
}

pub(super) fn serve(socket: &Path, containers: BTreeMap<String, Value>) -> TestResult<()> {
    let listener = UnixListener::bind(socket)?;
    let state: Arc<Objects> = Arc::new((containers, volumes()?));
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let state = Arc::clone(&state);
            tokio::spawn(async move { drop(respond(stream, &state).await) });
        }
    });
    Ok(())
}

async fn respond(mut stream: UnixStream, state: &Objects) -> TestResult<()> {
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
        ["info"] => Some(
            json!({"ID": ENGINE, "DockerRootDir": "/var/lib/docker", "OSType": "linux", "Architecture": "x86_64"}),
        ),
        ["containers", id, "json"] => state.0.get(*id).cloned(),
        ["volumes", name] => state.1.get(*name).cloned(),
        _ => None,
    };
    let (status, body) = found.map_or((404, json!({"message": "No such object"})), |body| {
        (200, body)
    });
    let body = body.to_string();
    let reply = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(reply.as_bytes()).await?;
    Ok(stream.shutdown().await?)
}
