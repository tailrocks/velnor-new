// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_host::HostError;
use velnor_runner_host::local::{StartFailure, start_pair};

use bollard::query_parameters::RemoveImageOptions;

use super::support::{Fixture, Program, TestResult, build_program, labelled, request, suffix};

#[tokio::test]
async fn local_pair_start_refuses_a_runner_that_exits_at_once_and_cleans_up() -> TestResult<()> {
    let fixture = Fixture::new().await?;
    let tag = format!("velnor-local-test-exiter:{}", suffix()?);
    let built = build_program(&fixture.docker, &tag, Program::Exit).await;
    let failed = match &built {
        Ok(exiter) => {
            let mut pair = fixture.request();
            pair.runner_image.clone_from(exiter);
            Some(Box::pin(start_pair(&fixture.docker, &pair)).await)
        }
        Err(_) => None,
    };
    let left = labelled(&fixture.docker, &fixture.environment).await;
    let removed = fixture
        .docker
        .remove_image(&tag, None::<RemoveImageOptions>, None)
        .await;
    fixture.finish().await?;
    built?;
    removed?;
    assert_eq!(failed, Some(Err(StartFailure::from(HostError::Ownership))));
    assert_eq!(left?, (0, 0));
    Ok(())
}

#[tokio::test]
async fn local_pair_start_failure_names_the_pair_when_cleanup_cannot_run() -> TestResult<()> {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("docker.sock");
    drop(std::os::unix::net::UnixListener::bind(&socket)?);
    let docker = velnor_runner_host::connect_unix(&socket.to_string_lossy())?;
    let image = format!("sha256:{}", "a".repeat(64));
    let failure = Box::pin(start_pair(
        &docker,
        &request("env-one", "velnor-local-net", &image),
    ))
    .await
    .err()
    .ok_or("start succeeded")?;
    assert_eq!(failure.error, HostError::Docker);
    assert!(failure.leftover.is_some_and(|name| !name.is_empty()));
    Ok(())
}

struct Broken;

impl std::io::Write for Broken {
    fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Supervisor {
    line: std::io::Cursor<Vec<u8>>,
    waited: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl std::io::Read for Supervisor {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.line.read(buffer)?;
        if read == 0 {
            self.waited.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(read)
    }
}

#[tokio::test]
async fn local_pair_one_shot_cleans_up_without_waiting_when_the_startup_line_cannot_be_written()
-> TestResult<()> {
    let fixture = Fixture::new().await?;
    let socket =
        std::env::var("DOCKER_HOST").unwrap_or_else(|_| "unix:///var/run/docker.sock".to_owned());
    let pair = fixture.request();
    let line = serde_json::json!({"docker": socket, "pair": {
        "environment": pair.environment, "network": pair.network, "platform": pair.platform,
        "runner_image": pair.runner_image, "dind_image": pair.dind_image,
        "sequence": pair.sequence,
        "runner_cpu_millicores": pair.runner_cpu_millicores,
        "runner_memory_bytes": pair.runner_memory_bytes,
        "dind_cpu_millicores": pair.dind_cpu_millicores,
        "dind_memory_bytes": pair.dind_memory_bytes,
    }});
    let waited = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut input = std::io::BufReader::new(Supervisor {
        line: std::io::Cursor::new(format!("{line}\n").into_bytes()),
        waited: waited.clone(),
    });
    let outcome = std::thread::spawn(move || {
        velnor_runner_host::local::run_one_pair(&mut input, &mut Broken)
    })
    .join()
    .map_err(|_| "one-shot panicked")?;
    let left = labelled(&fixture.docker, &fixture.environment).await;
    fixture.finish().await?;
    assert_eq!(outcome, Err(HostError::Path.into()));
    assert!(!waited.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(left?, (0, 0));
    Ok(())
}
