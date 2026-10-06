use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clap::CommandFactory;

use super::super::{ConnectRequest, connect_with};

#[cfg(target_os = "macos")]
const TEST_SERVICE: &str = "com.tailrocks.velnor.host.test";
const EMPTY_SERVICE: &str = "com.tailrocks.velnor.host.test.empty";

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
    if text.contains("--token") {
        return Err("token flag".to_owned());
    }
    Ok(())
}

#[test]
fn connect_empty_stdin_does_not_create_or_overwrite() -> Result<(), String> {
    let dir = TempDir::new("empty")?;
    #[cfg(target_os = "macos")]
    let _guard = KeychainItem {
        service: EMPTY_SERVICE,
        account: super::super::KEYCHAIN_ACCOUNT,
    };
    let missing = dir.path().join("host.toml");
    let mut empty = Cursor::new(b"");
    if connect_with(&mut empty, EMPTY_SERVICE, &request(dir.path())).is_ok() {
        return Err("empty stdin succeeded".to_owned());
    }
    if missing.exists() {
        return Err("created host.toml".to_owned());
    }
    std::fs::write(&missing, b"keep-me\n").map_err(|err| err.to_string())?;
    let mut again = Cursor::new(b"");
    if connect_with(&mut again, EMPTY_SERVICE, &request(dir.path())).is_ok() {
        return Err("empty overwrite succeeded".to_owned());
    }
    let body = std::fs::read(&missing).map_err(|err| err.to_string())?;
    if body.as_slice() != b"keep-me\n" {
        return Err("overwrote host.toml".to_owned());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn connect_writes_host_toml_without_the_token() -> Result<(), String> {
    let dir = TempDir::new("connect")?;
    let _guard = KeychainItem {
        service: TEST_SERVICE,
        account: super::super::KEYCHAIN_ACCOUNT,
    };
    let canary = b"canary-token\n";
    let mut input = Cursor::new(&canary[..]);
    connect_with(&mut input, TEST_SERVICE, &request(dir.path())).map_err(|err| err.to_string())?;
    let text =
        std::fs::read_to_string(dir.path().join("host.toml")).map_err(|err| err.to_string())?;
    if !text.contains("credential_ref = \"keychain:com.tailrocks.velnor.host/local\"") {
        return Err("missing credential_ref".to_owned());
    }
    if text.contains("canary-token") {
        return Err("toml contains token".to_owned());
    }
    let stored = security_framework::passwords::generic_password(
        security_framework::passwords::PasswordOptions::new_generic_password(
            TEST_SERVICE,
            super::super::KEYCHAIN_ACCOUNT,
        ),
    )
    .map_err(|_| "keychain read".to_owned())?;
    if stored.as_slice() != canary {
        return Err("keychain mismatch".to_owned());
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

#[cfg(target_os = "macos")]
struct KeychainItem {
    service: &'static str,
    account: &'static str,
}

#[cfg(target_os = "macos")]
impl Drop for KeychainItem {
    fn drop(&mut self) {
        let removed =
            security_framework::passwords::delete_generic_password(self.service, self.account);
        match removed {
            Ok(()) | Err(_) => {}
        }
    }
}
