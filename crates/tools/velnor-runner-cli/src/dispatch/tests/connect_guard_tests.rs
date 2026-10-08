use std::cell::Cell;
use std::io::Cursor;

use super::super::connect::{connect_with, connect_with_service_state};
use super::connect_common::{TempDir, allowed_events, allowed_workflow_paths, file_ops, request};
use crate::service::ControllerServiceState;
use velnor_runner_host::{HostConfig, HostPlatform};

#[test]
fn linux_connect_requires_workflow_paths_before_reading_or_storing_credential() -> Result<(), String>
{
    let dir = TempDir::new("missing-workflow-path")?;
    let path = dir.path().join("host.toml");
    let events = allowed_events();
    let empty_paths = Vec::new();
    let mut invalid = request(&path, &events, &empty_paths);
    invalid.allowed_workflow_paths = &empty_paths;
    let mut input = Cursor::new(b"must-not-be-consumed\n");
    let mut remote_called = false;
    let mut store_called = false;
    let result = connect_with(
        &mut input,
        &invalid,
        file_ops(),
        |_token, _config| {
            remote_called = true;
            Ok(())
        },
        |_config, _secret| {
            store_called = true;
            Ok(())
        },
    );
    if result.is_ok() || remote_called || store_called || input.position() != 0 || path.exists() {
        return Err("missing workflow allowlist was not rejected before credential use".to_owned());
    }
    Ok(())
}

#[test]
fn connect_requires_positive_stopped_service_state_before_any_credential_or_file_use()
-> Result<(), String> {
    for service_state in [
        ControllerServiceState::InUse,
        ControllerServiceState::Unknown,
    ] {
        let dir = TempDir::new("service-state")?;
        let path = dir.path().join("host.toml");
        let events = allowed_events();
        let workflow_paths = allowed_workflow_paths();
        let request = request(&path, &events, &workflow_paths);
        let mut input = Cursor::new(b"must-not-be-consumed\n");
        let remote_called = Cell::new(false);
        let store_called = Cell::new(false);
        let result = connect_with_service_state(
            &mut input,
            &request,
            service_state,
            file_ops(),
            |_, _| {
                remote_called.set(true);
                Ok(())
            },
            |_, _| {
                store_called.set(true);
                Ok(())
            },
        );
        if result.is_ok()
            || input.position() != 0
            || remote_called.get()
            || store_called.get()
            || path.exists()
        {
            return Err("non-stopped service state reached a connection side effect".to_owned());
        }
    }
    Ok(())
}

#[test]
fn invalid_workflow_path_is_rejected_before_credential_use() -> Result<(), String> {
    let dir = TempDir::new("invalid-workflow-path")?;
    let path = dir.path().join("host.toml");
    let events = allowed_events();
    let invalid_paths = vec![".github/workflows/../publish.yml".to_owned()];
    let request = request(&path, &events, &invalid_paths);
    let mut input = Cursor::new(b"must-not-be-consumed\n");
    let result = connect_with(
        &mut input,
        &request,
        file_ops(),
        |_, _| Ok(()),
        |_, _| Ok(()),
    );
    if result.is_ok() || input.position() != 0 || path.exists() {
        return Err("invalid workflow path reached credential processing".to_owned());
    }
    Ok(())
}

#[test]
fn linux_connect_requires_an_explicit_positive_drain_timeout() -> Result<(), String> {
    let dir = TempDir::new("missing-timeout")?;
    let path = dir.path().join("host.toml");
    let events = allowed_events();
    let workflow_paths = allowed_workflow_paths();
    let mut invalid = request(&path, &events, &workflow_paths);
    invalid.allowed_workflow_paths = &workflow_paths;
    invalid.drain_timeout_secs = None;
    let text = super::super::connect::sample_config_for(&invalid, HostPlatform::Linux);
    if text.is_ok() {
        return Err("Linux config accepted an arbitrary implicit drain timeout".to_owned());
    }
    Ok(())
}

#[test]
fn failed_credential_store_rolls_back_new_configuration() -> Result<(), String> {
    let dir = TempDir::new("store-failure")?;
    let path = dir.path().join("host.toml");
    let events = allowed_events();
    let workflow_paths = allowed_workflow_paths();
    let request = request(&path, &events, &workflow_paths);
    let mut input = Cursor::new(b"canary-token\n");
    let result = connect_with(
        &mut input,
        &request,
        file_ops(),
        |_, _| Ok(()),
        |_config, _secret| {
            Err(super::super::connect::ConnectError::Secret(
                velnor_runner_host::HostError::Keychain,
            ))
        },
    );
    if result.is_ok() || path.exists() {
        return Err("failed credential storage left a new config behind".to_owned());
    }
    Ok(())
}

#[test]
fn linux_connect_caps_drain_timeout_while_macos_keeps_legacy_range() {
    use velnor_runner_host::MAX_LINUX_DRAIN_TIMEOUT_SECS;

    let path = std::path::Path::new("/tmp/host.toml");
    let events = allowed_events();
    let workflow_paths = allowed_workflow_paths();
    let mut linux = request(path, &events, &workflow_paths);
    linux.host_platform = Some("linux");
    linux.image_profile = Some("ubuntu-26.04-amd64");
    linux.drain_timeout_secs = Some(MAX_LINUX_DRAIN_TIMEOUT_SECS + 1);
    assert!(super::super::connect::sample_config_for(&linux, HostPlatform::Linux).is_err());

    let mut macos = request(path, &events, &workflow_paths);
    macos.host_platform = Some("macos");
    macos.image_profile = None;
    macos.drain_timeout_secs = Some(MAX_LINUX_DRAIN_TIMEOUT_SECS + 1);
    let text = super::super::connect::sample_config_for(&macos, HostPlatform::Macos)
        .expect("macOS keeps its existing timeout range");
    let config = HostConfig::parse(&text).expect("generated macOS config parses");
    config
        .validate_for_host(HostPlatform::Macos)
        .expect("the Linux cap does not narrow macOS validation");
    assert_eq!(
        config.drain_timeout_secs().expect("timeout is present"),
        MAX_LINUX_DRAIN_TIMEOUT_SECS + 1
    );
}
