use std::cell::Cell;
use std::io::Cursor;

use clap::CommandFactory;

use super::super::connect::{ConnectRequest, connect_with, sample_config_for};
use super::connect_common::{
    TempDir, allowed_events, allowed_workflow_paths, expected_credential_ref, file_ops, request,
};
use crate::args::Cli;
use velnor_runner_host::{HostConfig, HostPlatform};

#[test]
fn connect_help_reads_stdin_and_has_no_token_flag() -> Result<(), String> {
    let mut command = Cli::command();
    let connect = command
        .find_subcommand_mut("connect")
        .ok_or("missing connect")?;
    let mut buffer = Vec::new();
    connect
        .write_long_help(&mut buffer)
        .map_err(|err| err.to_string())?;
    let text = String::from_utf8(buffer).map_err(|err| err.to_string())?;
    if !text.contains("stdin") || text.contains("--token") {
        return Err("connect help must identify stdin and omit token flags".to_owned());
    }
    Ok(())
}

#[test]
fn empty_stdin_does_not_create_or_overwrite_config() -> Result<(), String> {
    let dir = TempDir::new("empty")?;
    let path = dir.path().join("host.toml");
    let events = allowed_events();
    let workflow_paths = allowed_workflow_paths();
    let request = request(&path, &events, &workflow_paths);
    let mut empty = Cursor::new(b"");
    if connect_with(
        &mut empty,
        &request,
        file_ops(),
        |_, _| Ok(()),
        |_, _| Ok(()),
    )
    .is_ok()
    {
        return Err("empty stdin succeeded".to_owned());
    }
    if path.exists() {
        return Err("empty input created config".to_owned());
    }
    std::fs::write(&path, b"keep-me\n").map_err(|err| err.to_string())?;
    let mut again = Cursor::new(b"");
    if connect_with(
        &mut again,
        &request,
        file_ops(),
        |_, _| Ok(()),
        |_, _| Ok(()),
    )
    .is_ok()
    {
        return Err("empty overwrite succeeded".to_owned());
    }
    if std::fs::read(&path).map_err(|err| err.to_string())? != b"keep-me\n" {
        return Err("empty input overwrote config".to_owned());
    }
    Ok(())
}

#[test]
fn connect_validates_before_store_and_persists_secret_free_config() -> Result<(), String> {
    let dir = TempDir::new("connect")?;
    let path = dir.path().join("host.toml");
    let events = allowed_events();
    let workflow_paths = allowed_workflow_paths();
    let request = request(&path, &events, &workflow_paths);
    let canary = b"canary-token\n";
    let mut input = Cursor::new(&canary[..]);
    let validated = Cell::new(false);
    let stored = Cell::new(false);
    connect_with(
        &mut input,
        &request,
        file_ops(),
        |token, config| {
            if token != "canary-token" || config.github.repository != "example/repo" {
                return Err(super::super::connect::ConnectError::Rejected);
            }
            validated.set(true);
            Ok(())
        },
        |config, secret| {
            if !validated.get() || config.github.credential_ref != expected_credential_ref() {
                return Err(super::super::connect::ConnectError::Rejected);
            }
            stored.set(secret == canary);
            Ok(())
        },
    )
    .map_err(|error| error.to_string())?;
    let text = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let parsed = HostConfig::parse(&text).map_err(|err| err.to_string())?;
    let trust = parsed.job_trust_policy().map_err(|err| err.to_string())?;
    if !validated.get()
        || !stored.get()
        || text.contains("canary-token")
        || !text.contains("managed_by = \"velnor-host-connect-v1\"")
        || trust.allowed_workflow_paths != [".github/workflows/ci.yml".to_owned()]
    {
        return Err("connect did not safely validate and persist".to_owned());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .map_err(|err| err.to_string())?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o640 {
            return Err("Linux config mode does not match the service contract".to_owned());
        }
    }
    Ok(())
}

#[test]
fn original_macos_connect_options_still_generate_the_legacy_binding() -> Result<(), String> {
    let dir = TempDir::new("legacy-macos")?;
    let events = Vec::new();
    let config_path = dir.path().join("host.toml");
    let request = ConnectRequest {
        config_path: &config_path,
        repo: "tailrocks/velnor-new",
        scale_set: "ubuntu-26.04-scale-set",
        platform: "linux/amd64",
        host_platform: None,
        registration_scope: None,
        runner_group_id: None,
        runner_group_name: None,
        allowed_events: &events,
        allowed_workflow_paths: &[],
        image_profile: None,
        max_jobs: None,
        drain_timeout_secs: None,
        docker_context: None,
        endpoint: None,
    };
    let text = sample_config_for(&request, HostPlatform::Macos).map_err(|e| e.to_string())?;
    let config = HostConfig::parse(&text).map_err(|e| e.to_string())?;
    config
        .validate_for_host(HostPlatform::Macos)
        .map_err(|e| e.to_string())?;
    if config.host.platform.is_some()
        || config.github.runner_group_id.is_some()
        || config.trust.is_some()
        || config.host.drain_timeout_secs.is_some()
        || config.github.credential_ref != "keychain:com.tailrocks.velnor.host/velnor-host"
    {
        return Err("the original macOS binding surface changed".to_owned());
    }
    Ok(())
}
