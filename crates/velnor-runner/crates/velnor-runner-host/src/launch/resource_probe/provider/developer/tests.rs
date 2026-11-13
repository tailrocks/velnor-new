// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::sync::Arc;

use bollard::models::SystemInfo;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::prepare_worker_images;
use crate::config::DeveloperImages;
use crate::error::HostError;
use crate::journal::EngineContext;
use crate::launch::SelectedEngineContext;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const SOURCE: &str = "0123456789abcdef0123456789abcdef01234567";
const PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

fn id(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}

fn image(fill: char, kind: &str, architecture: &str, source: &str) -> (String, Value) {
    let mut config = json!({
        "Env": [PATH_ENV],
        "Entrypoint": [format!("/usr/local/bin/velnor-{kind}-entrypoint")],
        "Labels": {
            "org.opencontainers.image.created": "2026-10-06T20:38:12Z",
            "org.opencontainers.image.description": "Ubuntu",
            "org.opencontainers.image.title": "ubuntu",
            "org.opencontainers.image.version": "26.04",
            "io.velnor.image.kind": kind,
            "org.opencontainers.image.revision": source,
        },
    });
    if kind == "runner" {
        config["User"] = json!("runner");
        config["WorkingDir"] = json!("/home/runner");
    }
    (
        id(fill),
        json!({"Id": id(fill), "Os": "linux", "Architecture": architecture, "Config": config}),
    )
}

async fn serve(stream: &mut UnixStream, images: &HashMap<String, Value>) -> TestResult<()> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        request.extend_from_slice(&chunk[..read]);
    }
    let text = String::from_utf8_lossy(&request);
    let path = text.split_whitespace().nth(1).unwrap_or_default();
    let found = images
        .iter()
        .find(|(id, _)| path.contains(&format!("/images/{id}/json")));
    let (status, body) = match found {
        Some((_, body)) => ("200 OK", body.to_string()),
        None => ("404 Not Found", r#"{"message":"No such image"}"#.to_owned()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn prepare(
    platform: &str,
    images: Vec<(String, Value)>,
) -> TestResult<Result<(String, String), HostError>> {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("docker.sock");
    let listener = UnixListener::bind(&socket)?;
    let images: Arc<HashMap<String, Value>> = Arc::new(images.into_iter().collect());
    let served = Arc::clone(&images);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let images = Arc::clone(&served);
            tokio::spawn(async move {
                let _served = serve(&mut stream, &images).await;
            });
        }
    });
    let docker = crate::docker_client::connect_unix(&format!("unix://{}", socket.display()))?;
    let selected = SelectedEngineContext::capture(
        &EngineContext {
            engine_id: "engine-1".to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
            platform: platform.to_owned(),
        },
        &SystemInfo {
            id: Some("engine-1".to_owned()),
            os_type: Some("linux".to_owned()),
            architecture: Some("aarch64".to_owned()),
            docker_root_dir: Some("/var/lib/docker".to_owned()),
            ..SystemInfo::default()
        },
    )?;
    let developer: DeveloperImages = toml::from_str(&format!(
        "runner = \"{}\"\ndind = \"{}\"\nresource_probe = \"{}\"\n",
        id('a'),
        id('b'),
        id('c')
    ))?;
    Ok(prepare_worker_images(&docker, &selected, &developer)
        .await
        .map(|verified| {
            (
                verified.platform().to_owned(),
                verified.source_sha().to_owned(),
            )
        }))
}

#[tokio::test]
async fn local_arm64_images_become_verified_worker_images() -> TestResult<()> {
    let result = prepare(
        "linux/arm64",
        vec![
            image('a', "runner", "arm64", SOURCE),
            image('b', "dind", "arm64", SOURCE),
        ],
    )
    .await?;
    assert_eq!(result, Ok(("linux/arm64".to_owned(), SOURCE.to_owned())));
    Ok(())
}

#[tokio::test]
async fn local_images_that_do_not_match_the_host_are_refused() -> TestResult<()> {
    let other = "fedcba9876543210fedcba9876543210fedcba98";
    let mut changed_config = image('a', "runner", "arm64", SOURCE);
    changed_config.1["Config"]["Cmd"] = json!(["sh"]);
    for (case, runner, dind) in [
        (
            "amd64 image on an arm64 host",
            image('a', "runner", "amd64", SOURCE),
            image('b', "dind", "amd64", SOURCE),
        ),
        (
            "runner and DinD from different sources",
            image('a', "runner", "arm64", SOURCE),
            image('b', "dind", "arm64", other),
        ),
        (
            "runner image with another runtime config",
            changed_config.clone(),
            image('b', "dind", "arm64", SOURCE),
        ),
        (
            "DinD image under the runner ID",
            (id('a'), image('b', "dind", "arm64", SOURCE).1),
            image('b', "dind", "arm64", SOURCE),
        ),
    ] {
        let result = prepare("linux/arm64", vec![runner, dind]).await?;
        assert!(result.is_err(), "{case} accepted: {result:?}");
    }
    Ok(())
}
