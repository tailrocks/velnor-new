//! `connect` stores stdin through an injected Keychain writer and writes `host.toml`.

use std::cell::RefCell;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clap::{CommandFactory, Parser};

use super::{ConnectRequest, connect_with, connect_with_store};

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
        "--runner-cpu-millicores",
        "--runner-memory-bytes",
        "--dind-cpu-millicores",
        "--dind-memory-bytes",
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
        "credential_ref = \"keychain:{TEST_SERVICE}/local\""
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
    ] {
        if !text.contains(field) {
            return Err(format!("missing {field}"));
        }
    }
    let stored = stored.into_inner().ok_or("store callback was not called")?;
    if stored.0 != TEST_SERVICE || stored.1 != "local" || stored.2.as_slice() != canary {
        return Err("stored credential differs from the TOML reference".to_owned());
    }
    Ok(())
}

#[test]
fn idempotent_connect_preserves_the_configured_credential_reference() -> Result<(), String> {
    let dir = TempDir::new("preserve-reference")?;
    let custom_reference = r#"keychain:org.example.host/chain\\quoted\"argos"#;
    let initial = super::sample_config(
        &super::SampleConfig {
            repo: "example/repo",
            scale_set: "ubuntu-26.04-scale-set",
            platform: "linux/amd64",
            max_jobs: 1,
            docker_context: Some("orbstack"),
            endpoint: Some("unix:///var/run/docker.sock"),
            runner_cpu_millicores: 1_000,
            runner_memory_bytes: 2_147_483_648,
            dind_cpu_millicores: 3_000,
            dind_memory_bytes: 6_442_450_944,
        },
        &velnor_runner_host::KeychainReference::parse(&format!("keychain:{TEST_SERVICE}/local"))
            .map_err(|error| error.to_string())?,
    )
    .replace(&format!("keychain:{TEST_SERVICE}/local"), custom_reference);
    std::fs::create_dir_all(dir.path()).map_err(|err| err.to_string())?;
    std::fs::write(dir.path().join("host.toml"), initial).map_err(|err| err.to_string())?;

    let stored = RefCell::new(None);
    let mut input = Cursor::new(b"replacement-token\n");
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

    let selected = stored.into_inner().ok_or("store callback was not called")?;
    if selected.0 != "org.example.host" || selected.1 != "chain\\quoted\"argos" {
        return Err("idempotent connect ignored the configured Keychain pair".to_owned());
    }
    let persisted =
        std::fs::read_to_string(dir.path().join("host.toml")).map_err(|err| err.to_string())?;
    if !persisted.contains(&format!("credential_ref = \"{custom_reference}\"")) {
        return Err("idempotent connect replaced the configured reference".to_owned());
    }
    Ok(())
}

fn request(state: &Path) -> ConnectRequest<'_> {
    ConnectRequest {
        state,
        repo: "example/repo",
        scale_set: "ubuntu-26.04-scale-set",
        platform: "linux/amd64",
        max_jobs: Some(1),
        docker_context: Some("orbstack"),
        endpoint: Some("unix:///var/run/docker.sock"),
        runner_cpu_millicores: 1_000,
        runner_memory_bytes: 2_147_483_648,
        dind_cpu_millicores: 3_000,
        dind_memory_bytes: 6_442_450_944,
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

#[test]
fn connect_requires_all_four_resource_limits() {
    let complete = [
        "velnor-host",
        "connect",
        "--repo",
        "example/repo",
        "--scale-set",
        "ubuntu-26.04-scale-set",
        "--platform",
        "linux/amd64",
        "--runner-cpu-millicores",
        "1000",
        "--runner-memory-bytes",
        "2147483648",
        "--dind-cpu-millicores",
        "3000",
        "--dind-memory-bytes",
        "6442450944",
    ];
    assert!(crate::Cli::try_parse_from(complete).is_ok());
    let missing_memory = [
        "velnor-host",
        "connect",
        "--repo",
        "example/repo",
        "--scale-set",
        "ubuntu-26.04-scale-set",
        "--platform",
        "linux/amd64",
        "--runner-cpu-millicores",
        "1000",
        "--runner-memory-bytes",
        "2147483648",
        "--dind-cpu-millicores",
        "3000",
    ];
    assert!(crate::Cli::try_parse_from(missing_memory).is_err());
}
