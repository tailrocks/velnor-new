// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use super::{ControlServer, IO_TIMEOUT};
use crate::config::HostConfig;
use crate::controller_ipc::client::wait_for_drain;
use crate::controller_ipc::wire::{Request, Response, SOCKET_NAME, Target};
use crate::ipc::encode_frame;
use crate::journal::Journal;

type TestResult = Result<(), Box<dyn Error>>;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const CONFIG: &str = "schema = 2\n\
    [[connections]]\n\
    connection_id = \"lab\"\n\
    repository_id = 42\n\
    repository = \"tailrocks/velnor-lab\"\n\
    credential_ref = \"keychain:com.tailrocks.velnor.host/lab\"\n\
    [connections.scale_set]\n\
    name = \"ubuntu-26.04-arm-scale-set\"\n\
    labels = [\"velnor\", \"ubuntu-26.04-arm-scale-set\"]\n\
    [connections.source_policy]\n\
    mode = \"branch_restricted\"\n\
    branches = [\"main\"]\n\
    tag_prefixes = []\n\
    events = [\"push\"]\n\
    pull_request_rules = []\n\
    [host]\n\
    max_jobs = 1\n\
    [host.resources]\n\
    runner_cpu_millicores = 2000\n\
    runner_memory_bytes = 4294967296\n\
    dind_cpu_millicores = 1000\n\
    dind_memory_bytes = 2147483648\n\
    operational_memory_reserve_bytes = 1073741824\n\
    [docker]\n\
    context = \"orbstack\"\n\
    platform = \"linux/arm64\"\n\
    endpoint = \"unix:///var/run/docker.sock\"\n";

struct Running {
    dir: tempfile::TempDir,
    journal: Journal,
    server: ControlServer,
}

async fn drained_host() -> Result<Running, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let journal = Journal::open(&dir.path().join("journal.db")).await?;
    assert!(journal.is_drained().await?);
    let config = HostConfig::parse(CONFIG)?;
    let (server, _) = ControlServer::start(
        dir.path(),
        journal.clone(),
        &config,
        String::new(),
        String::new(),
    )?;
    std::fs::write(dir.path().join("drain"), TOKEN)?;
    Ok(Running {
        dir,
        journal,
        server,
    })
}

fn drain_status_frame() -> Result<Vec<u8>, Box<dyn Error>> {
    let request = Request::DrainStatus {
        target: Target::Host,
        token: TOKEN.to_owned(),
    };
    let payload = request.encode().map_err(|()| "encode")?;
    Ok(encode_frame(&payload)?)
}

fn read_response(stream: &mut UnixStream) -> Result<Response, Box<dyn Error>> {
    let mut prefix = [0; 4];
    stream.read_exact(&mut prefix)?;
    let mut bytes = vec![0; usize::try_from(u32::from_be_bytes(prefix))?];
    stream.read_exact(&mut bytes)?;
    Ok(Response::decode_with_limit(&bytes, 4096).map_err(|()| "decode")?)
}

fn connect(state: &Path) -> Result<UnixStream, Box<dyn Error>> {
    let stream = UnixStream::connect(state.join(SOCKET_NAME))?;
    stream.set_read_timeout(Some(IO_TIMEOUT * 2))?;
    Ok(stream)
}

async fn drain_value_after(delay: Duration, drained: bool) -> Result<Option<bool>, Box<dyn Error>> {
    let running = drained_host().await?;
    if !drained {
        running.journal.begin("job", "run-1").await?;
    }
    let mut stream = connect(running.dir.path())?;
    thread::sleep(delay);
    stream.write_all(&drain_status_frame()?)?;
    Ok(read_response(&mut stream)?.value)
}

#[tokio::test]
async fn a_request_written_after_the_accept_pause_reaches_the_handler() -> TestResult {
    assert_eq!(
        drain_value_after(Duration::from_millis(100), true).await?,
        Some(true)
    );
    assert_eq!(
        drain_value_after(Duration::from_millis(100), false).await?,
        Some(false)
    );
    assert_eq!(drain_value_after(Duration::ZERO, true).await?, Some(true));
    Ok(())
}

#[tokio::test]
async fn the_drain_client_reaches_the_handler_through_the_socket() -> TestResult {
    let running = drained_host().await?;
    assert!(wait_for_drain(running.dir.path(), Duration::from_secs(1))?);
    drop(running.server);
    Ok(())
}

#[tokio::test]
async fn a_peer_that_never_writes_waits_for_the_read_timeout_before_the_server_gives_up()
-> TestResult {
    let running = drained_host().await?;
    let mut stream = connect(running.dir.path())?;
    let started = Instant::now();
    let mut byte = [0; 1];
    let read = stream.read(&mut byte)?;
    assert!(started.elapsed() >= Duration::from_secs(4));
    assert!(read == 0 || read == 1);
    Ok(())
}
