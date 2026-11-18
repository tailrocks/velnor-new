// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Write};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bollard::models::NetworkCreateRequest;
use bollard::query_parameters::TagImageOptions;
use velnor_runner_host::local::start_pair;

use super::proxy::{Faults, Proxy, Wait};
use super::support::{
    Endpoint, Fixture, Guard, Program, TestResult, build_program, labelled, suffix,
};

struct Ambient {
    address: String,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    task: Option<std::thread::JoinHandle<()>>,
}

impl Ambient {
    fn new() -> TestResult<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = format!("tcp://{}", listener.local_addr()?);
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let count = Arc::clone(&requests);
        let stopped = Arc::clone(&stop);
        let task = std::thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                if let Ok((mut client, _)) = listener.accept() {
                    count.fetch_add(1, Ordering::SeqCst);
                    let _timeout = client.set_read_timeout(Some(std::time::Duration::from_secs(1)));
                    let _read = client.read(&mut [0; 8192]);
                    let _sent = client.write_all(b"HTTP/1.1 500 Poisoned\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        });
        Ok(Self {
            address,
            requests,
            stop,
            task: Some(task),
        })
    }

    fn child(&self, test: &str, role: &str, explicit: Option<&str>) -> TestResult<Output> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["--exact", test, "--nocapture"])
            .env("VELNOR_ENDPOINT_CHILD", role)
            .env("DOCKER_HOST", &self.address)
            .env_remove("VELNOR_TEST_DOCKER_HOST");
        if let Some(explicit) = explicit {
            command.env("VELNOR_TEST_DOCKER_HOST", explicit);
        }
        Ok(command.output()?)
    }
}

impl Drop for Ambient {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(task) = self.task.take() {
            let _joined = task.join();
        }
    }
}

fn transcript(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[tokio::test]
async fn absent_endpoint_skips_and_invalid_endpoint_fails_without_ambient_requests()
-> TestResult<()> {
    if std::env::var_os("VELNOR_ENDPOINT_CHILD").is_some() {
        if let Some(fixture) = Fixture::new().await? {
            fixture.finish().await?;
            return Err("unexpected Docker fixture".into());
        }
        return Ok(());
    }
    let ambient = Ambient::new()?;
    let test =
        "endpoint::absent_endpoint_skips_and_invalid_endpoint_fails_without_ambient_requests";
    let skipped = ambient.child(test, "select", None)?;
    assert!(skipped.status.success(), "{}", transcript(&skipped));
    assert!(transcript(&skipped).contains("SKIP Docker integration test"));
    for invalid in [
        "",
        "not-an-endpoint",
        "tcp://127.0.0.1:0",
        "unix://relative",
    ] {
        let failed = ambient.child(test, "select", Some(invalid))?;
        assert!(
            !failed.status.success(),
            "{invalid:?}: {}",
            transcript(&failed)
        );
        assert!(!transcript(&failed).contains("SKIP Docker integration test"));
    }
    assert_eq!(ambient.requests.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn guest_scenarios_skip_without_a_linux_hook_before_contacting_docker() -> TestResult<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let ambient = Ambient::new()?;
    let test = "refusals::local_job_start_check_refuses_a_run_whose_event_ref_or_sha_differ";
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", test, "--nocapture"])
        .env("VELNOR_TEST_DOCKER_HOST", &ambient.address)
        .env_remove("VELNOR_TEST_JOB_HOOK_BINARY")
        .env_remove("VELNOR_TEST_JOB_HOOK_ROOTFS")
        .output()?;
    let transcript = transcript(&output);
    assert!(output.status.success(), "{transcript}");
    assert!(
        transcript.contains(
            "SKIP Docker integration test: set VELNOR_TEST_JOB_HOOK_BINARY to a Linux velnor-job-start-hook binary"
        ),
        "{transcript}"
    );
    assert_eq!(ambient.requests.load(Ordering::SeqCst), 0);
    Ok(())
}

async fn selected_fixture() -> TestResult<()> {
    let fixture = Fixture::new()
        .await?
        .ok_or("explicit endpoint was skipped")?;
    let docker = fixture.docker.clone();
    let environment = fixture.environment.clone();
    let network = fixture.network.clone();
    let image = fixture.image.clone();
    let tag = fixture.image_tag().to_owned();
    let alias_suffix = suffix()?;
    let alias = format!("velnor-local-test-alias:{alias_suffix}");
    let mut alias_guard = Guard::new(
        docker.clone(),
        &format!("test-alias-{alias_suffix}"),
        &alias,
        &format!("velnor-local-test-alias-{alias_suffix}"),
    );
    docker
        .tag_image(
            &image,
            Some(TagImageOptions {
                repo: Some("velnor-local-test-alias".to_owned()),
                tag: Some(alias_suffix),
            }),
        )
        .await?;
    let proxy = Proxy::start(&fixture.endpoint, Faults::new(|_| Wait::Forward, 0))?;
    proxy.docker.ping().await?;
    Box::pin(start_pair(&proxy.docker, &fixture.request())).await?;
    assert_eq!(labelled(&docker, &environment).await?, (2, 5));
    drop(fixture);
    assert_eq!(labelled(&docker, &environment).await?, (0, 0));
    assert!(docker.inspect_network(&network, None).await.is_err());
    let removed = docker.inspect_image(&tag).await;
    let retained_alias = docker.inspect_image(&alias).await;
    let retained_digest = docker.inspect_image(&image).await;
    alias_guard.finish().await?;
    assert!(matches!(
        removed,
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        })
    ));
    assert_eq!(retained_alias?.id.as_deref(), Some(image.as_str()));
    assert_eq!(retained_digest?.id.as_deref(), Some(image.as_str()));
    Ok(())
}

async fn retained_guard(endpoint: &str) -> TestResult<()> {
    let docker = Endpoint::parse(endpoint)?.docker()?;
    let suffix = suffix()?;
    let environment = format!("test-{suffix}");
    let image = format!("velnor-local-test-retained:{suffix}");
    let network = format!("velnor-local-test-{suffix}");
    build_program(&docker, &image, Program::Sleep).await?;
    docker
        .create_network(NetworkCreateRequest {
            name: network.clone(),
            ..Default::default()
        })
        .await?;
    drop(Guard::new(docker.clone(), &environment, &image, &network));
    let network_gone = docker.inspect_network(&network, None).await.is_err();
    let image_gone = docker.inspect_image(&image).await.is_err();
    Guard::new(docker, &environment, &image, &network)
        .finish()
        .await?;
    assert!(
        network_gone && image_gone,
        "Guard did not retain its selected daemon"
    );
    Ok(())
}

#[tokio::test]
async fn explicit_endpoint_serves_fixture_proxy_and_guard_without_ambient_requests()
-> TestResult<()> {
    if let Ok(role) = std::env::var("VELNOR_ENDPOINT_CHILD") {
        return match role.as_str() {
            "fixture" => selected_fixture().await,
            "guard" => retained_guard(&std::env::var("VELNOR_RETAINED_ENDPOINT")?).await,
            _ => Err("unknown child role".into()),
        };
    }
    let Some(_) = Endpoint::selected()? else {
        return Ok(());
    };
    let explicit = std::env::var("VELNOR_TEST_DOCKER_HOST")?;
    let ambient = Ambient::new()?;
    let test =
        "endpoint::explicit_endpoint_serves_fixture_proxy_and_guard_without_ambient_requests";
    let fixture = ambient.child(test, "fixture", Some(&explicit))?;
    assert!(fixture.status.success(), "{}", transcript(&fixture));
    let guard = Command::new(std::env::current_exe()?)
        .args(["--exact", test, "--nocapture"])
        .env("VELNOR_ENDPOINT_CHILD", "guard")
        .env("VELNOR_RETAINED_ENDPOINT", &explicit)
        .env("VELNOR_TEST_DOCKER_HOST", &ambient.address)
        .env("DOCKER_HOST", &ambient.address)
        .output()?;
    assert!(guard.status.success(), "{}", transcript(&guard));
    assert_eq!(ambient.requests.load(Ordering::SeqCst), 0);
    Ok(())
}
