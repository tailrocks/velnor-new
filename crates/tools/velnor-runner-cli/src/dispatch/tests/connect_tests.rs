use std::cell::Cell;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clap::CommandFactory;

use super::super::connect::{
    ConnectFileOps, ConnectRequest, connect_with, connect_with_service_state, sample_config_for,
};
use crate::args::Cli;
use crate::service::ControllerServiceState;
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

fn file_ops() -> ConnectFileOps {
    use super::super::connect::ConnectError;

    ConnectFileOps {
        read: |path| match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                std::fs::read_to_string(path)
                    .map(Some)
                    .map_err(|_| ConnectError::Write)
            }
            Ok(_) => Err(ConnectError::Config),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(ConnectError::Write),
        },
        persist: |path, text| {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut options = std::fs::OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .mode(if cfg!(target_os = "linux") {
                    0o640
                } else {
                    0o600
                });
            let mut file = options.open(path).map_err(|_| ConnectError::Write)?;
            file.write_all(text.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|_| ConnectError::Write)
        },
        remove: |path, expected| {
            let contents = std::fs::read_to_string(path).map_err(|_| ConnectError::Write)?;
            if contents != expected {
                return Err(ConnectError::Write);
            }
            std::fs::remove_file(path).map_err(|_| ConnectError::Write)
        },
    }
}

fn allowed_events() -> Vec<String> {
    vec!["push".to_owned(), "pull_request".to_owned()]
}

fn allowed_workflow_paths() -> Vec<String> {
    vec![".github/workflows/ci.yml".to_owned()]
}

fn request<'a>(
    config_path: &'a Path,
    events: &'a [String],
    workflow_paths: &'a [String],
) -> ConnectRequest<'a> {
    ConnectRequest {
        config_path,
        repo: "example/repo",
        scale_set: target_set(),
        platform: "linux/amd64",
        host_platform: Some(target_host()),
        registration_scope: Some("repository"),
        runner_group_id: Some(1),
        runner_group_name: Some("Default"),
        allowed_events: events,
        allowed_workflow_paths: workflow_paths,
        image_profile: target_profile(),
        max_jobs: Some(1),
        drain_timeout_secs: Some(1800),
        docker_context: Some("system"),
        endpoint: Some("unix:///var/run/docker.sock"),
    }
}

fn expected_credential_ref() -> &'static str {
    if cfg!(target_os = "linux") {
        "systemd-credential:github-token"
    } else {
        "keychain:com.tailrocks.velnor.host/velnor-host"
    }
}

fn target_host() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else {
        "macos"
    }
}

fn target_set() -> &'static str {
    if cfg!(target_os = "linux") {
        "ubuntu-24.04-scale-set"
    } else {
        "ubuntu-26.04-scale-set"
    }
}

fn target_profile() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        Some("ubuntu-24.04-amd64")
    } else {
        None
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, String> {
        static TICK: AtomicU64 = AtomicU64::new(0);
        let n = TICK.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-connect-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir(&path).map_err(|err| err.to_string())?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}
