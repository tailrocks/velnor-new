// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::SystemInfo;

use super::SelectedEngineContext;
use crate::journal::EngineContext;

fn capture(platform: &str, architecture: &str) -> Option<String> {
    let selected = EngineContext {
        engine_id: "engine-1".to_owned(),
        endpoint: "unix:///var/run/docker.sock".to_owned(),
        platform: platform.to_owned(),
    };
    let info = SystemInfo {
        id: Some("engine-1".to_owned()),
        os_type: Some("linux".to_owned()),
        architecture: Some(architecture.to_owned()),
        docker_root_dir: Some("/var/lib/docker".to_owned()),
        ..SystemInfo::default()
    };
    SelectedEngineContext::capture(&selected, &info)
        .ok()
        .map(|context| format!("{} {}", context.platform(), context.daemon_architecture()))
}

#[test]
fn arm64_runs_only_on_a_native_arm64_daemon() {
    assert_eq!(
        capture("linux/arm64", "aarch64").as_deref(),
        Some("linux/arm64 aarch64")
    );
    assert_eq!(capture("linux/arm64", "x86_64"), None);
}

#[test]
fn amd64_keeps_running_on_either_daemon() {
    assert_eq!(
        capture("linux/amd64", "x86_64").as_deref(),
        Some("linux/amd64 x86_64")
    );
    assert_eq!(
        capture("linux/amd64", "aarch64").as_deref(),
        Some("linux/amd64 aarch64")
    );
}
