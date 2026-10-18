//! Keep the connection config until its journal work ends.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::DRAIN_WAIT_BUDGET;
use super::disconnect::disconnect_with_detach;
use super::drain::{drain_with, remove_flag, remove_marker};
use super::marker::{MarkerToken, marker_token};
use std::cell::RefCell;
use velnor_runner_host::{ConnectionId, HostConfig, HostError};

const CONFIG: &str = concat!(
    "schema = 2\n",
    "[[connections]]\n",
    "connection_id = \"essential-mac\"\n",
    "repository_id = 123\n",
    "repository = \"tailrocks/velnor-new\"\n",
    "credential_ref = \"keychain:com.tailrocks.velnor.host/essential-mac\"\n",
    "[connections.scale_set]\n",
    "name = \"essential-scale-set\"\n",
    "labels = [\"velnor\", \"essential-scale-set\"]\n",
    "[connections.source_policy]\n",
    "mode = \"branch_restricted\"\n",
    "branches = [\"main\"]\n",
    "tag_prefixes = []\n",
    "events = [\"push\"]\n",
    "pull_request_rules = []\n",
    "[[connections]]\n",
    "connection_id = \"java-integration\"\n",
    "repository_id = 456\n",
    "repository = \"ChainArgos/java-monorepo\"\n",
    "credential_ref = \"keychain:com.tailrocks.velnor.host/java-integration\"\n",
    "[connections.scale_set]\n",
    "name = \"java-scale-set\"\n",
    "labels = [\"velnor\", \"java-scale-set\"]\n",
    "[connections.source_policy]\n",
    "mode = \"branch_restricted\"\n",
    "branches = [\"sea/integration\"]\n",
    "tag_prefixes = []\n",
    "events = [\"push\"]\n",
    "pull_request_rules = []\n",
    "[host]\n",
    "max_jobs = 2\n",
    "[host.resources]\n",
    "runner_cpu_millicores = 1000\n",
    "runner_memory_bytes = 2147483648\n",
    "dind_cpu_millicores = 3000\n",
    "dind_memory_bytes = 6442450944\n",
    "operational_memory_reserve_bytes = 1073741824\n",
    "[docker]\n",
    "context = \"orbstack\"\n",
    "platform = \"linux/amd64\"\n",
    "endpoint = \"unix:///var/run/docker.sock\"\n",
);

#[test]
fn disconnect_without_wait_records_drain_and_keeps_config() -> Result<(), String> {
    let dir = TempDir::new()?;
    write_config(dir.path(), CONFIG)?;
    let result = disconnect_with_detach(
        dir.path(),
        "essential-mac",
        false,
        |_, _, _| Err(HostError::Journal),
        |_, _, _| Err(HostError::Journal),
    );
    let config = read_config(dir.path())?;
    let id = connection_id("essential-mac")?;
    if result != std::process::ExitCode::SUCCESS
        || !drain_marker(dir.path(), &id).is_file()
        || !binding_snapshot(dir.path(), &id).is_file()
        || config.connections().len() != 2
        || config.connection(&id).is_none()
    {
        return Err("async disconnect did not keep its binding draining".to_owned());
    }
    Ok(())
}

#[test]
fn disconnect_timeout_keeps_the_binding_and_drain_marker() -> Result<(), String> {
    let dir = TempDir::new()?;
    write_config(dir.path(), CONFIG)?;
    let result = disconnect_with_detach(
        dir.path(),
        "essential-mac",
        true,
        |state, id, timeout| {
            if state != dir.path() || id.as_str() != "essential-mac" || timeout != DRAIN_WAIT_BUDGET
            {
                return Err(HostError::Config);
            }
            Ok(false)
        },
        |_, _, _| Err(HostError::Journal),
    );
    let config = read_config(dir.path())?;
    let id = connection_id("essential-mac")?;
    if result == std::process::ExitCode::SUCCESS
        || config.connection(&id).is_none()
        || !drain_marker(dir.path(), &id).is_file()
    {
        return Err("timed out disconnect removed an active binding".to_owned());
    }
    Ok(())
}

#[test]
fn disconnect_wait_removes_only_the_selected_local_binding() -> Result<(), String> {
    let dir = TempDir::new()?;
    write_config(dir.path(), CONFIG)?;
    let result = disconnect_with_detach(
        dir.path(),
        "essential-mac",
        true,
        |state, id, timeout| {
            if state == dir.path() && id.as_str() == "essential-mac" && timeout == DRAIN_WAIT_BUDGET
            {
                Ok(true)
            } else {
                Err(HostError::Config)
            }
        },
        |state, binding, timeout| {
            if state == dir.path()
                && binding.connection_id.as_str() == "essential-mac"
                && timeout == DRAIN_WAIT_BUDGET
            {
                Ok(true)
            } else {
                Err(HostError::Config)
            }
        },
    );
    let config = read_config(dir.path())?;
    let removed = connection_id("essential-mac")?;
    let remaining = connection_id("java-integration")?;
    if result != std::process::ExitCode::SUCCESS
        || config.connections().len() != 1
        || config.connection(&removed).is_some()
        || config.connection(&remaining).is_none()
        || config.host.max_jobs != 2
        || config.docker.context != "orbstack"
        || !drain_marker(dir.path(), &removed).is_file()
        || !binding_snapshot(dir.path(), &removed).is_file()
        || !drain_proof(dir.path(), &removed).is_file()
    {
        return Err("disconnect changed a shared setting or another binding".to_owned());
    }
    Ok(())
}

#[test]
fn disconnect_wait_can_remove_the_last_local_binding() -> Result<(), String> {
    let dir = TempDir::new()?;
    let mut config = HostConfig::parse(CONFIG).map_err(|error| error.to_string())?;
    let second = connection_id("java-integration")?;
    config
        .remove_connection(&second)
        .map_err(|error| error.to_string())?;
    let one = config.to_toml().map_err(|error| error.to_string())?;
    write_config(dir.path(), &one)?;
    let result = disconnect_with_detach(
        dir.path(),
        "essential-mac",
        true,
        |_, _, _| Ok(true),
        |_, _, _| Ok(true),
    );
    let config = read_config(dir.path())?;
    if result != std::process::ExitCode::SUCCESS || !config.connections().is_empty() {
        return Err("last connection was not detached".to_owned());
    }
    Ok(())
}

#[test]
fn disconnect_does_not_remove_a_binding_changed_while_waiting() -> Result<(), String> {
    let dir = TempDir::new()?;
    write_config(dir.path(), CONFIG)?;
    let result = disconnect_with_detach(
        dir.path(),
        "essential-mac",
        true,
        |state, _, _| {
            let path = state.join("host.toml");
            let changed = std::fs::read_to_string(&path)
                .map_err(|_| HostError::Journal)?
                .replacen("branches = [\"main\"]", "branches = [\"release\"]", 1);
            std::fs::write(path, changed).map_err(|_| HostError::Journal)?;
            Ok(true)
        },
        |_, _, _| Ok(true),
    );
    let config = read_config(dir.path())?;
    let id = connection_id("essential-mac")?;
    let connection = config
        .connection(&id)
        .ok_or("changed connection was removed")?;
    if result == std::process::ExitCode::SUCCESS
        || connection.source_policy.branches.len() != 1
        || connection.source_policy.branches[0] != "release"
        || !drain_marker(dir.path(), &id).is_file()
    {
        return Err("disconnect removed a changed binding".to_owned());
    }
    Ok(())
}

#[test]
fn disconnect_does_not_report_removing_a_binding_removed_by_another_caller() -> Result<(), String> {
    let dir = TempDir::new()?;
    write_config(dir.path(), CONFIG)?;
    let result = disconnect_with_detach(
        dir.path(),
        "essential-mac",
        true,
        |state, id, _| {
            let path = state.join("host.toml");
            let text = std::fs::read_to_string(&path).map_err(|_| HostError::Journal)?;
            let mut config = HostConfig::parse(&text)?;
            config.remove_connection(id)?;
            let text = config.to_toml().map_err(|_| HostError::Config)?;
            std::fs::write(path, text).map_err(|_| HostError::Journal)?;
            Ok(true)
        },
        |_, _, _| Ok(true),
    );
    if result == std::process::ExitCode::SUCCESS {
        return Err("disconnect claimed an effect from another caller".to_owned());
    }
    Ok(())
}

#[test]
fn disconnect_unknown_connection_does_not_write_a_marker() -> Result<(), String> {
    let dir = TempDir::new()?;
    write_config(dir.path(), CONFIG)?;
    let result = disconnect_with_detach(
        dir.path(),
        "missing",
        true,
        |_, _, _| Err(HostError::Journal),
        |_, _, _| Err(HostError::Journal),
    );
    let id = connection_id("missing")?;
    if result == std::process::ExitCode::SUCCESS || drain_marker(dir.path(), &id).exists() {
        return Err("unknown connection changed state".to_owned());
    }
    Ok(())
}

fn write_config(state: &Path, text: &str) -> Result<(), String> {
    std::fs::create_dir_all(state).map_err(|error| error.to_string())?;
    std::fs::write(state.join("host.toml"), text).map_err(|error| error.to_string())
}

fn read_config(state: &Path) -> Result<HostConfig, String> {
    let text =
        std::fs::read_to_string(state.join("host.toml")).map_err(|error| error.to_string())?;
    HostConfig::parse(&text).map_err(|error| error.to_string())
}

fn connection_id(value: &str) -> Result<ConnectionId, String> {
    ConnectionId::parse(value).map_err(|error| error.to_string())
}

fn drain_marker(state: &Path, connection_id: &ConnectionId) -> PathBuf {
    state.join("connection-drain").join(connection_id.as_str())
}

fn binding_snapshot(state: &Path, connection_id: &ConnectionId) -> PathBuf {
    state
        .join("connection-drain")
        .join(format!("{}~binding", connection_id.as_str()))
}

fn drain_proof(state: &Path, connection_id: &ConnectionId) -> PathBuf {
    state
        .join("connection-drain")
        .join(format!("{}~drained", connection_id.as_str()))
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let index = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-disconnect-{}-{index}", std::process::id()));
        if path.exists() {
            return Err("temp path exists".to_owned());
        }
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[path = "disconnect_tests/global_drain.rs"]
mod global_drain_tests;
#[path = "disconnect_tests/recovery.rs"]
mod recovery_tests;
