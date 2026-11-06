// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    ffi::{OsStr, OsString},
    io::{Read, Write},
    net::TcpStream,
    path::Path,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use velnor_actions_mise::{CheckDeadline, PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_orchestrator::registry_gateway::RegistryGateway;

#[path = "registry_gateway_public/support.rs"]
mod support;
use support::{Layout, Registry, RegistryBehavior, lock_error};

#[path = "registry_gateway_public/protocol.rs"]
mod protocol;

const REPOSITORY: &str = "ghcr.io/tailrocks/velnor-job";
const AUTHORIZATION: &str = "Zml4dHVyZTpmaXh0dXJl";
const TOKEN: &str = "fixture-token-0123456789";

#[test]
fn pinned_crane_pushes_real_oci_bytes_through_the_gateway() -> Result<(), Box<dyn Error>> {
    let registry = Arc::new(Registry {
        behavior: RegistryBehavior::CdnRead,
        ..Registry::default()
    });
    let gateway = RegistryGateway::with_io(
        REPOSITORY,
        AUTHORIZATION.into(),
        registry.clone(),
        CheckDeadline::after(Duration::from_secs(90))?,
    )?;
    let fixture = Layout::new()?;
    let directory = tempfile::tempdir()?;
    let layout = directory.path().join("layout");
    fixture.write(&layout)?;
    let reference = format!("{}:sha-fixture", gateway.repository(REPOSITORY));

    let layout_path = layout.to_str().ok_or("layout path is not UTF-8")?;
    crane(
        gateway.config().as_path(),
        &["push", layout_path, &reference],
    )?;
    crane(
        gateway.config().as_path(),
        &["push", layout_path, &reference],
    )?;
    let digest_reference = format!(
        "{}@{}",
        gateway.repository(REPOSITORY),
        fixture.manifest_digest
    );
    crane(
        gateway.config().as_path(),
        &["tag", &digest_reference, "version-fixture"],
    )?;
    let layer_reference = format!(
        "{}@{}",
        gateway.repository(REPOSITORY),
        fixture.layer_digest
    );
    assert_eq!(
        crane_output(gateway.config().as_path(), &["blob", &layer_reference])?,
        fixture.layer
    );

    let state = registry.state.lock().map_err(lock_error)?;
    assert_eq!(
        state.tags.get("sha-fixture"),
        Some(&fixture.manifest_digest)
    );
    assert_eq!(
        state.tags.get("version-fixture"),
        Some(&fixture.manifest_digest)
    );
    assert_eq!(
        state.manifests.get(&fixture.manifest_digest),
        Some(&fixture.manifest)
    );
    assert_eq!(state.blobs, fixture.blobs);
    assert_eq!(state.token_requests, 1);
    assert_eq!(state.bad_requests, Vec::<String>::new());
    Ok(())
}

#[test]
fn gateway_rejects_foreign_upload_locations_before_crane_can_follow_them()
-> Result<(), Box<dyn Error>> {
    for location in [
        "https://127.0.0.1:9/steal",
        "https://trap.invalid/v2/tailrocks/velnor-job/blobs/uploads/{upload}",
        "/v2/tailrocks/velnor-job/blobs/uploads/%2e%2e/steal",
    ] {
        let registry = Arc::new(Registry {
            upload_location: Some(location.into()),
            ..Registry::default()
        });
        let gateway = RegistryGateway::with_io(
            REPOSITORY,
            AUTHORIZATION.into(),
            registry.clone(),
            CheckDeadline::after(Duration::from_secs(90))?,
        )?;
        let fixture = Layout::new()?;
        let directory = tempfile::tempdir()?;
        let layout = directory.path().join("layout");
        fixture.write(&layout)?;
        let reference = format!("{}:sha-fixture", gateway.repository(REPOSITORY));

        assert!(
            crane(
                gateway.config().as_path(),
                &[
                    "push",
                    layout.to_str().ok_or("layout path is not UTF-8")?,
                    &reference,
                ],
            )
            .is_err()
        );

        let state = registry.state.lock().map_err(lock_error)?;
        assert!(state.tags.is_empty());
        assert!(state.blobs.is_empty());
        assert_eq!(state.bad_requests, Vec::<String>::new());
    }
    Ok(())
}

#[test]
fn gateway_rejects_token_redirects_without_contacting_the_redirect_target()
-> Result<(), Box<dyn Error>> {
    let registry = Arc::new(Registry {
        behavior: RegistryBehavior::TokenRedirect,
        ..Registry::default()
    });
    let gateway = RegistryGateway::with_io(
        REPOSITORY,
        AUTHORIZATION.into(),
        registry.clone(),
        CheckDeadline::after(Duration::from_secs(90))?,
    )?;
    let fixture = Layout::new()?;
    let directory = tempfile::tempdir()?;
    let layout = directory.path().join("layout");
    fixture.write(&layout)?;
    let reference = format!("{}:sha-fixture", gateway.repository(REPOSITORY));

    assert!(
        crane(
            gateway.config().as_path(),
            &[
                "push",
                layout.to_str().ok_or("layout path is not UTF-8")?,
                &reference,
            ],
        )
        .is_err()
    );

    let state = registry.state.lock().map_err(lock_error)?;
    assert!(state.token_requests > 0);
    assert!(state.tags.is_empty());
    assert_eq!(state.bad_requests, Vec::<String>::new());
    Ok(())
}

#[test]
fn gateway_rejects_oversized_tokens_challenges_and_registry_redirects() -> Result<(), Box<dyn Error>>
{
    for registry in [
        Registry {
            behavior: RegistryBehavior::OversizedToken,
            ..Registry::default()
        },
        Registry {
            behavior: RegistryBehavior::Challenge,
            ..Registry::default()
        },
        Registry {
            behavior: RegistryBehavior::ManifestRedirect,
            ..Registry::default()
        },
        Registry {
            behavior: RegistryBehavior::WriteRedirect,
            ..Registry::default()
        },
    ] {
        assert_publication_fails(registry)?;
    }
    Ok(())
}

#[test]
fn gateway_drop_drains_external_io_until_deadline() -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    let deadline = CheckDeadline::after(Duration::from_secs(2))?;
    let registry = Arc::new(Registry {
        token_deadline: Some(deadline),
        ..Registry::default()
    });
    let gateway =
        RegistryGateway::with_io(REPOSITORY, AUTHORIZATION.into(), registry.clone(), deadline)?;
    let local = gateway.repository(REPOSITORY);
    let (address, _) = local.split_once('/').ok_or("bad reference")?;
    let address = address.to_owned();
    let client = std::thread::spawn(move || {
        protocol::raw_gateway_address(
            &address,
            "GET",
            "/v2/tailrocks/velnor-job/manifests/fixture",
        )
        .map_err(|error| error.to_string())
    });
    assert!(registry.token_gate.wait_started(deadline)?);
    assert_eq!(
        registry
            .active_requests
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    let (drop_started_tx, drop_started_rx) = mpsc::sync_channel(0);
    let (drop_finished_tx, drop_finished_rx) = mpsc::sync_channel(0);
    let dropper = std::thread::spawn(move || {
        if drop_started_tx.send(()).is_err() {
            return false;
        }
        drop(gateway);
        drop_finished_tx.send(()).is_ok()
    });
    drop_started_rx.recv_timeout(deadline.remaining()?)?;
    assert_eq!(
        registry
            .active_requests
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
    assert!(matches!(
        drop_finished_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop_finished_rx.recv_timeout(Duration::from_secs(3))?;
    assert!(dropper.join().map_err(|_| "gateway dropper panicked")?);
    assert_eq!(
        registry
            .active_requests
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    let response = client
        .join()
        .map_err(|_| "client thread panicked")?
        .map_err(std::io::Error::other)?;

    let state = registry.state.lock().map_err(lock_error)?;
    assert!(!response.starts_with("HTTP/1.1 2"));
    assert!(state.tags.is_empty());
    assert_eq!(state.token_requests, 1);
    assert!(started.elapsed() < Duration::from_secs(4));
    Ok(())
}

fn assert_publication_fails(registry: Registry) -> Result<(), Box<dyn Error>> {
    let registry = Arc::new(registry);
    let gateway = RegistryGateway::with_io(
        REPOSITORY,
        AUTHORIZATION.into(),
        registry.clone(),
        CheckDeadline::after(Duration::from_secs(90))?,
    )?;
    let fixture = Layout::new()?;
    let directory = tempfile::tempdir()?;
    let layout = directory.path().join("layout");
    fixture.write(&layout)?;
    let reference = format!("{}:sha-fixture", gateway.repository(REPOSITORY));

    assert!(
        crane(
            gateway.config().as_path(),
            &[
                "push",
                layout.to_str().ok_or("layout path is not UTF-8")?,
                &reference,
            ],
        )
        .is_err()
    );

    let state = registry.state.lock().map_err(lock_error)?;
    assert!(state.tags.is_empty());
    assert_eq!(state.bad_requests, Vec::<String>::new());
    Ok(())
}

fn crane(config: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    crane_output(config, args).map(|_| ())
}

fn crane_output(config: &Path, args: &[&str]) -> Result<Vec<u8>, Box<dyn Error>> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Crane],
        OsStr::new("crane"),
        args.iter().map(OsString::from).collect(),
    )?;
    let command = request.command(&ToolCatalog::pinned())?.with_env(&[
        (
            OsString::from("DOCKER_CONFIG"),
            config.as_os_str().to_owned(),
        ),
        (
            OsString::from("NO_PROXY"),
            OsString::from("127.0.0.1,localhost"),
        ),
        (
            OsString::from("no_proxy"),
            OsString::from("127.0.0.1,localhost"),
        ),
    ])?;
    let (output, _) =
        command.run_quiescent_until(1024 * 1024, CheckDeadline::after(Duration::from_secs(90))?)?;
    if !output.success {
        return Err(format!("crane failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(output.stdout)
}
