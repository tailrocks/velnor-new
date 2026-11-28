// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;
use velnor_runner_host::{
    ConnectionId, HostError, LaunchMode, RustRunnerFailure, host_toml_sha256,
};

use super::connect::{ConnectError, ConnectRequest};
use super::connect_tests::{FakeKeychain, INVENTORY, SOURCE_SHA, connect, request};
use super::connection_state::connection_drain_marker;
use super::rust_mode::{self, Reconnect};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(0))
}

fn write_census(state: &Path, updated: u64, admitting: bool, request: Option<&str>) -> TestResult {
    let census = json!({
        "config_sha256": host_toml_sha256(state).ok_or("config hash")?,
        "updated_unix_ms": updated,
        "drain": null,
        "reconciled": true,
        "connections": [{
            "id": "conn-test-1", "slots": 0, "drain": null,
            "request": request, "admitting": admitting,
        }],
    });
    let staging = state.join("census.staging");
    std::fs::write(&staging, census.to_string())?;
    std::fs::rename(staging, state.join("rust-runner-census.json"))?;
    Ok(())
}

fn request_of(state: &Path) -> Option<String> {
    std::fs::read_to_string(state.join("census-request/conn-test-1")).ok()
}

fn request_within(
    state: &Path,
    budget: Duration,
    ready: impl Fn() -> bool,
) -> Result<String, String> {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(nonce) = request_of(state).filter(|_| ready()) {
            return Ok(nonce);
        }
        if Instant::now() >= deadline {
            return Err("the command wrote no census request".to_owned());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn rust_request<'a>(state: &'a Path, inventory: &'a Path, artifact: &'a str) -> ConnectRequest<'a> {
    ConnectRequest {
        scale_set: "velnor-mac",
        scale_set_labels: vec!["velnor".to_owned(), "velnor-mac".to_owned()],
        platform: "linux/arm64",
        launch_mode: LaunchMode::RustRunner,
        job_authority_inventory: Some(inventory),
        job_authority_generator_source_sha: Some(SOURCE_SHA),
        job_authority_generator_artifact_sha256: Some(artifact),
        job_authority_generation: Some(1),
        ..request(state, None)
    }
}

#[test]
fn reconnecting_a_drained_connection_waits_for_a_census_that_admits_it() -> TestResult {
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?;
    let inventory = state.join("inventory.json");
    std::fs::write(&inventory, INVENTORY)?;
    let artifact = "b".repeat(64);
    let keychain = FakeKeychain::default();
    let request = rust_request(&state, &inventory, &artifact);
    assert_eq!(connect(&keychain, &request, "token-0"), Ok(()));
    assert_eq!(
        rust_mode::disconnect(&state, "conn-test-1", false),
        ExitCode::SUCCESS
    );
    let marker = connection_drain_marker(&state, &ConnectionId::parse("conn-test-1")?);
    assert!(marker.exists());
    write_census(&state, now_ms() - 10_000, true, None)?;

    let published = Arc::new(AtomicBool::new(false));
    let controller = {
        let (state, marker, published) = (state.clone(), marker.clone(), Arc::clone(&published));
        std::thread::spawn(move || -> Result<(), String> {
            let nonce = request_within(&state, Duration::from_secs(5), || !marker.exists())?;
            std::thread::sleep(Duration::from_millis(400));
            published.store(true, Ordering::SeqCst);
            write_census(&state, now_ms(), true, Some(&nonce)).map_err(|error| error.to_string())
        })
    };
    assert_eq!(connect(&keychain, &request, "token-1"), Ok(()));
    assert!(published.load(Ordering::SeqCst));
    assert!(!marker.exists());
    controller.join().map_err(|_| "controller stand-in")??;
    Ok(())
}

#[test]
fn a_census_that_keeps_the_connection_closed_does_not_admit_a_reconnect() -> TestResult {
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?;
    let inventory = state.join("inventory.json");
    std::fs::write(&inventory, INVENTORY)?;
    let artifact = "b".repeat(64);
    let keychain = FakeKeychain::default();
    let request = rust_request(&state, &inventory, &artifact);
    assert_eq!(connect(&keychain, &request, "token-0"), Ok(()));
    assert_eq!(
        rust_mode::disconnect(&state, "conn-test-1", false),
        ExitCode::SUCCESS
    );
    let reconnect = Reconnect::begin(&request).ok_or("a drained connection")?;
    let closed = std::thread::scope(|scope| -> Result<_, String> {
        let state = &state;
        let controller = scope.spawn(move || {
            let nonce = request_within(state, Duration::from_secs(5), || true)?;
            write_census(state, now_ms(), false, Some(&nonce)).map_err(|error| error.to_string())
        });
        let closed = reconnect.admitted_within(state, Duration::from_secs(1));
        controller.join().map_err(|_| "controller stand-in")??;
        Ok(closed)
    })?;
    assert_eq!(
        closed,
        Err(ConnectError::Lifecycle(HostError::RustRunner(
            RustRunnerFailure::ReconnectNotAdmitted
        )))
    );
    Ok(())
}

#[test]
fn a_census_that_does_not_echo_the_request_of_the_command_does_not_admit_a_reconnect() -> TestResult
{
    let temp = tempfile::tempdir()?;
    let state = temp.path().canonicalize()?;
    let inventory = state.join("inventory.json");
    std::fs::write(&inventory, INVENTORY)?;
    let artifact = "b".repeat(64);
    let keychain = FakeKeychain::default();
    let request = rust_request(&state, &inventory, &artifact);
    assert_eq!(connect(&keychain, &request, "token-0"), Ok(()));
    assert_eq!(
        rust_mode::disconnect(&state, "conn-test-1", false),
        ExitCode::SUCCESS
    );
    let reconnect = Reconnect::begin(&request).ok_or("a drained connection")?;
    write_census(
        &state,
        now_ms(),
        true,
        Some("0123456789abcdef0123456789abcdef"),
    )?;
    assert_eq!(
        reconnect.admitted_within(&state, Duration::from_secs(1)),
        Err(ConnectError::Lifecycle(HostError::RustRunner(
            RustRunnerFailure::ReconnectNotAdmitted
        )))
    );
    Ok(())
}
