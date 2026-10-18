//! `connect` stores stdin through an injected Keychain writer and writes `host.toml`.

use std::cell::RefCell;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clap::CommandFactory;

use super::connect::{
    ConnectError, ConnectRequest, connect_with, connect_with_store,
    connect_with_store_and_reactivate, connection_from,
};
use super::connect_config::sample_config;
use super::connection_state::write_reactivation_proof;
use super::disconnect::disconnect_with_detach;
use super::marker::marker_token;

const TEST_SERVICE: &str = "com.tailrocks.velnor.host.test";

#[test]
fn connect_help_reads_stdin_and_has_no_token_flag() -> Result<(), String> {
    let mut command = crate::Cli::command();
    let connect = command
        .find_subcommand_mut("connect")
        .ok_or("missing connect")?;
    let mut buffer = Vec::new();
    connect
        .write_long_help(&mut buffer)
        .map_err(|err| err.to_string())?;
    let text = String::from_utf8(buffer).map_err(|err| err.to_string())?;
    if !text.contains("stdin") || !text.contains("not a flag") {
        return Err("help omits stdin".to_owned());
    }
    for flag in [
        "--connection-id",
        "--repository-id",
        "--scale-set-label",
        "--source-policy-mode",
        "--event",
        "--branch",
        "--runner-cpu-millicores",
        "--runner-memory-bytes",
        "--dind-cpu-millicores",
        "--dind-memory-bytes",
        "--operational-memory-reserve-bytes",
    ] {
        if !text.contains(flag) {
            return Err(format!("missing required resource flag {flag}"));
        }
    }
    if text.contains("--token") {
        return Err("token flag".to_owned());
    }
    Ok(())
}

#[test]
fn connect_empty_stdin_does_not_create_or_overwrite() -> Result<(), String> {
    let dir = TempDir::new("empty")?;
    let missing = dir.path().join("host.toml");
    let mut empty = Cursor::new(b"");
    if connect_with(&mut empty, TEST_SERVICE, &request(dir.path())).is_ok() {
        return Err("empty stdin succeeded".to_owned());
    }
    if missing.exists() {
        return Err("created host.toml".to_owned());
    }
    std::fs::write(&missing, b"keep-me\n").map_err(|err| err.to_string())?;
    let mut again = Cursor::new(b"");
    if connect_with(&mut again, TEST_SERVICE, &request(dir.path())).is_ok() {
        return Err("empty overwrite succeeded".to_owned());
    }
    let body = std::fs::read(&missing).map_err(|err| err.to_string())?;
    if body.as_slice() != b"keep-me\n" {
        return Err("overwrote host.toml".to_owned());
    }
    Ok(())
}

#[test]
fn connect_stores_and_writes_the_same_configured_reference() -> Result<(), String> {
    let dir = TempDir::new("connect")?;
    let canary = b"canary-token\n";
    let mut input = Cursor::new(&canary[..]);
    let stored = RefCell::new(None);
    connect_with_store(
        &mut input,
        TEST_SERVICE,
        &request(dir.path()),
        |service, account, secret| {
            *stored.borrow_mut() = Some((service.to_owned(), account.to_owned(), secret.to_vec()));
            Ok(())
        },
    )
    .map_err(|err| err.to_string())?;
    let text =
        std::fs::read_to_string(dir.path().join("host.toml")).map_err(|err| err.to_string())?;
    if !text.contains(&format!(
        "credential_ref = \"keychain:{TEST_SERVICE}/example-repo\""
    )) {
        return Err("missing credential_ref".to_owned());
    }
    if text.contains("canary-token") {
        return Err("toml contains token".to_owned());
    }
    for field in [
        "runner_cpu_millicores = 1000",
        "runner_memory_bytes = 2147483648",
        "dind_cpu_millicores = 3000",
        "dind_memory_bytes = 6442450944",
        "operational_memory_reserve_bytes = 1073741824",
    ] {
        if !text.contains(field) {
            return Err(format!("missing {field}"));
        }
    }
    let stored = stored.into_inner().ok_or("store callback was not called")?;
    if stored.0 != TEST_SERVICE || stored.1 != "example-repo" || stored.2.as_slice() != canary {
        return Err("stored credential differs from the TOML reference".to_owned());
    }
    Ok(())
}

#[test]
fn connect_adds_one_isolated_binding_without_changing_the_shared_host() -> Result<(), String> {
    let dir = TempDir::new("add-connection")?;
    let mut first_input = Cursor::new(b"first-token\n");
    connect_with_store(
        &mut first_input,
        TEST_SERVICE,
        &request(dir.path()),
        |_, _, _| Ok(()),
    )
    .map_err(|error| error.to_string())?;

    let mut second = request(dir.path());
    second.connection_id = "java-integration";
    second.repo = "ChainArgos/java-monorepo";
    second.repository_id = 456;
    second.scale_set = "java-sea-integration";
    second.scale_set_labels = vec!["velnor".to_owned(), "java-sea-integration".to_owned()];
    second.branches = vec!["sea/integration".to_owned()];
    let second_credential = RefCell::new(None);
    let mut second_input = Cursor::new(b"second-token\n");
    connect_with_store(
        &mut second_input,
        TEST_SERVICE,
        &second,
        |service, account, secret| {
            *second_credential.borrow_mut() =
                Some((service.to_owned(), account.to_owned(), secret.to_vec()));
            Ok(())
        },
    )
    .map_err(|error| error.to_string())?;

    let text =
        std::fs::read_to_string(dir.path().join("host.toml")).map_err(|error| error.to_string())?;
    let config = velnor_runner_host::HostConfig::parse(&text).map_err(|error| error.to_string())?;
    let first_id = velnor_runner_host::ConnectionId::parse("example-repo")
        .map_err(|error| error.to_string())?;
    let second_id = velnor_runner_host::ConnectionId::parse("java-integration")
        .map_err(|error| error.to_string())?;
    if config.connections().len() != 2
        || config.connection(&first_id).is_none()
        || config.connection(&second_id).is_none()
        || config.host.max_jobs != 1
        || config.docker.context != "orbstack"
    {
        return Err("connection add changed shared host settings".to_owned());
    }
    let credential = second_credential
        .into_inner()
        .ok_or("second credential was not stored")?;
    if credential.0 != TEST_SERVICE
        || credential.1 != "java-integration"
        || credential.2.as_slice() != b"second-token\n"
    {
        return Err("second connection used the wrong credential".to_owned());
    }
    if text.contains("first-token") || text.contains("second-token") {
        return Err("TOML contains credential material".to_owned());
    }
    Ok(())
}

#[test]
fn connect_does_not_replace_an_invalid_existing_config() -> Result<(), String> {
    let dir = TempDir::new("invalid-config")?;
    let path = dir.path().join("host.toml");
    std::fs::write(&path, b"invalid = [\n").map_err(|error| error.to_string())?;
    let mut input = Cursor::new(b"token\n");
    let stored = RefCell::new(false);
    let result = connect_with_store(&mut input, TEST_SERVICE, &request(dir.path()), |_, _, _| {
        *stored.borrow_mut() = true;
        Ok(())
    });
    let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    if result.is_ok() || *stored.borrow() || text != "invalid = [\n" {
        return Err("connect replaced an invalid config".to_owned());
    }
    Ok(())
}

#[test]
fn connect_rejects_a_legacy_drain_marker_without_binding_proof() -> Result<(), String> {
    let dir = TempDir::new("draining-connection")?;
    let marker = dir.path().join("connection-drain/example-repo");
    std::fs::create_dir_all(
        marker
            .parent()
            .ok_or("missing connection drain directory")?,
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(&marker, b"1").map_err(|error| error.to_string())?;
    let mut input = Cursor::new(b"token\n");
    let stored = RefCell::new(false);
    let result = connect_with_store_and_reactivate(
        &mut input,
        TEST_SERVICE,
        &request(dir.path()),
        |_, _, _| {
            *stored.borrow_mut() = true;
            Ok(())
        },
        |_, _| Ok(true),
    );
    if !matches!(result, Err(ConnectError::Rejected)) || *stored.borrow() {
        return Err("connect accepted an unproven drain marker".to_owned());
    }
    Ok(())
}

#[test]
fn idempotent_connect_preserves_the_configured_credential_reference() -> Result<(), String> {
    let dir = TempDir::new("preserve-reference")?;
    let custom_reference = r#"keychain:org.example.host/chain"quoted"#;
    let request = request(dir.path());
    let connection = connection_from(
        &request,
        velnor_runner_host::ConnectionId::parse(request.connection_id)
            .map_err(|error| error.to_string())?,
        velnor_runner_host::KeychainReference::parse(custom_reference)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let initial = sample_config(&request, &connection);
    std::fs::create_dir_all(dir.path()).map_err(|err| err.to_string())?;
    std::fs::write(dir.path().join("host.toml"), initial).map_err(|err| err.to_string())?;

    let stored = RefCell::new(None);
    let mut input = Cursor::new(b"replacement-token\n");
    connect_with_store(
        &mut input,
        TEST_SERVICE,
        &request,
        |service, account, secret| {
            *stored.borrow_mut() = Some((service.to_owned(), account.to_owned(), secret.to_vec()));
            Ok(())
        },
    )
    .map_err(|err| err.to_string())?;

    let selected = stored.into_inner().ok_or("store callback was not called")?;
    if selected.0 != "org.example.host" || selected.1 != "chain\"quoted" {
        return Err("idempotent connect ignored the configured Keychain pair".to_owned());
    }
    let persisted =
        std::fs::read_to_string(dir.path().join("host.toml")).map_err(|err| err.to_string())?;
    let config =
        velnor_runner_host::HostConfig::parse(&persisted).map_err(|error| error.to_string())?;
    let id = velnor_runner_host::ConnectionId::parse(request.connection_id)
        .map_err(|error| error.to_string())?;
    if config
        .connection(&id)
        .map(|connection| connection.credential_ref.to_string())
        .as_deref()
        != Some(custom_reference)
    {
        return Err("idempotent connect replaced the configured reference".to_owned());
    }
    Ok(())
}

fn request(state: &Path) -> ConnectRequest<'_> {
    ConnectRequest {
        state,
        connection_id: "example-repo",
        repo: "example/repo",
        repository_id: 123,
        scale_set: "ubuntu-26.04-scale-set",
        scale_set_labels: vec!["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
        source_policy_mode: "branch-restricted",
        events: vec!["push".to_owned()],
        branches: vec!["main".to_owned()],
        tag_prefixes: Vec::new(),
        pull_request_rules: Vec::new(),
        platform: "linux/amd64",
        max_jobs: Some(1),
        docker_context: Some("orbstack"),
        endpoint: Some("unix:///var/run/docker.sock"),
        runner_cpu_millicores: 1_000,
        runner_memory_bytes: 2_147_483_648,
        dind_cpu_millicores: 3_000,
        dind_memory_bytes: 6_442_450_944,
        operational_memory_reserve_bytes: 1_073_741_824,
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, String> {
        static TICK: AtomicU64 = AtomicU64::new(0);
        let n = TICK.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-connect-{label}-{}-{n}", std::process::id()));
        if path.exists() {
            return Err("temp path exists".to_owned());
        }
        std::fs::create_dir_all(&path).map_err(|err| err.to_string())?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) | Err(_) => {}
        }
    }
}

#[path = "connect_tests/cli_args.rs"]
mod cli_args_tests;
#[path = "connect_tests/policy.rs"]
mod policy_tests;
#[path = "connect_tests/reconnect.rs"]
mod reconnect_tests;
