use super::*;
use serde_json::json;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use velnor_actions_contract_config::config::HostContainerProfile;

#[cfg(unix)]
mod inventory;
#[cfg(unix)]
mod unix;

fn docker_profile(socket: &Path, socket_uid: u32) -> HostContainerProfile {
    serde_json::from_value(json!({
        "provider": "docker",
        "context": "ci",
        "socket_path": socket.display().to_string(),
        "socket_uid": socket_uid,
        "cli": {
            "path": "/usr/bin/docker",
            "sha256": "0".repeat(64),
            "version": "27.0.0",
            "build": "fixture"
        },
        "daemon": {
            "version": "27.0.0",
            "platform": "linux_x64",
            "operating_system": "Docker fixture",
            "identity_policy": "execution_scoped"
        }
    }))
    .expect("docker profile")
}

fn orb_profile(socket: &Path, runtime: &Path, uid: u32) -> HostContainerProfile {
    serde_json::from_value(json!({
        "provider": "orb_stack",
        "context": "ci",
        "socket_path": socket.display().to_string(),
        "cli": {
            "path": "/usr/bin/docker",
            "sha256": "0".repeat(64),
            "version": "27.0.0",
            "build": "fixture"
        },
        "daemon": {
            "version": "27.0.0",
            "platform": "linux_x64",
            "operating_system": "OrbStack fixture",
            "identity_policy": "execution_scoped"
        },
        "sdk": {
            "app_bundle_path": "/Applications/OrbStack.app",
            "bundle_id": "com.orbstack.OrbStack",
            "team_id": "TEAM123456",
            "version": "1.0.0",
            "build": "1",
            "info_plist_sha256": "0".repeat(64),
            "main_executable_path": "Contents/MacOS/OrbStack",
            "main_executable_sha256": "0".repeat(64),
            "cli_bundle_path": "/Applications/OrbStack.app/Contents/CLI.app",
            "source_tree_sha256": "0".repeat(64),
            "owned_tree_sha256": "0".repeat(64),
            "cli_relative_path": "Contents/MacOS/orbctl",
            "cli_sha256": "0".repeat(64),
            "cli_version": "1.0.0",
            "cli_build": "1",
            "cli_commit": "0".repeat(40),
            "runtime_dir": runtime.display().to_string(),
            "runtime_uid": uid
        }
    }))
    .expect("orb profile")
}

fn socket(path: &Path) -> UnixListener {
    UnixListener::bind(path).expect("unix socket")
}

fn home_uid(path: &Path) -> u32 {
    fs::metadata(path).expect("home metadata").uid()
}

fn orb_uid(runtime: &Path, socket: &Path) -> u32 {
    let uid = home_uid(runtime);
    if uid > 0 {
        return uid;
    }
    let uid = 501;
    for path in [
        runtime,
        &runtime.join("status"),
        &runtime.join("vmgr.version"),
        socket,
    ] {
        set_fixture_owner(path, uid);
    }
    uid
}

fn set_fixture_owner(path: &Path, uid: u32) {
    rustix::fs::chownat(
        rustix::fs::CWD,
        path,
        Some(rustix::fs::Uid::from_raw(uid)),
        None,
        rustix::fs::AtFlags::empty(),
    )
    .expect("fixture owner");
}
