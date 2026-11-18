// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::engine_identity;
use super::{JournalFact, MAX_JOURNAL_ROWS, journal_mark};
use crate::config::HostConfig;
use crate::error::HostError;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const ENGINE_ID: &str = "engine-test";
const MAX_INFO_BYTES: usize = 8 * 1024 * 1024;
const CONFIG: &str = r#"schema = 2
[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

fn info_body(total: usize) -> Vec<u8> {
    let mut body = format!(
        r#"{{"ID":"{ENGINE_ID}","DockerRootDir":"/var/lib/docker","OSType":"linux","Architecture":"x86_64"}}"#
    )
    .into_bytes();
    if total > body.len() {
        body.resize(total, b' ');
    }
    body
}

async fn serve_info(mut stream: UnixStream, body: &[u8]) -> TestResult {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        request.extend_from_slice(&chunk[..read]);
    }
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(stream.shutdown().await?)
}

fn start_daemon(socket: &Path, body: Vec<u8>) -> TestResult {
    let listener = UnixListener::bind(socket)?;
    let body = std::sync::Arc::new(body);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let body = std::sync::Arc::clone(&body);
            tokio::spawn(async move { drop(serve_info(stream, &body).await) });
        }
    });
    Ok(())
}

async fn identity_from(body: Vec<u8>) -> TestResult<Option<(String, String)>> {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("docker.sock");
    start_daemon(&socket, body)?;
    let endpoint = format!("unix://{}", socket.display());
    let config = HostConfig::parse(&CONFIG.replace("unix:///docker.sock", &endpoint))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    Ok(engine_identity(&config, deadline).await)
}

#[tokio::test]
async fn a_small_info_body_yields_the_engine_identity() -> TestResult {
    let identity = identity_from(info_body(0)).await?;
    assert_eq!(
        identity.map(|(engine, _)| engine).as_deref(),
        Some(ENGINE_ID)
    );
    Ok(())
}

#[tokio::test]
async fn an_info_body_over_the_cap_yields_no_engine_identity() -> TestResult {
    assert!(
        identity_from(info_body(MAX_INFO_BYTES + 1))
            .await?
            .is_none()
    );
    Ok(())
}

async fn journal_with(path: &Path, occupying_first: bool, rows: usize) -> Result<(), HostError> {
    let db = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let conn = db.connect().map_err(|_| HostError::Journal)?;
    conn.execute(
        "CREATE TABLE intents (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, subject TEXT NOT NULL, state TEXT NOT NULL, cleanup_proven INTEGER NOT NULL DEFAULT 0)",
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    let values: Vec<String> = (0..rows)
        .map(|index| {
            let proven = i32::from(!(occupying_first && index == 0));
            format!("('launch', 's{index}', 'pending', {proven})")
        })
        .collect();
    conn.execute(
        &format!(
            "INSERT INTO intents (kind, subject, state, cleanup_proven) VALUES {}",
            values.join(",")
        ),
        (),
    )
    .await
    .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn mark(rows: usize) -> Result<JournalFact, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("journal.db");
    journal_with(&path, true, rows).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    Ok(journal_mark(&path, deadline).await)
}

#[tokio::test]
async fn an_occupying_first_row_does_not_hide_an_over_cap_journal()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(mark(MAX_JOURNAL_ROWS + 1).await?, JournalFact::Unreadable);
    Ok(())
}

#[tokio::test]
async fn one_occupying_row_alone_is_occupied_not_unreadable()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(mark(1).await?, JournalFact::Occupied);
    Ok(())
}

#[tokio::test]
async fn an_occupying_first_row_at_exactly_the_cap_is_occupied()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(mark(MAX_JOURNAL_ROWS).await?, JournalFact::Occupied);
    Ok(())
}
