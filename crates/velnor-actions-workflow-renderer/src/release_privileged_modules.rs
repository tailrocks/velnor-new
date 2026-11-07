// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;

pub(crate) struct BootstrapSource {
    pub key: &'static str,
    pub constant: &'static str,
    pub marker: &'static str,
    pub source: &'static str,
}

macro_rules! source {
    ($key:literal, $constant:literal, $marker:literal, $file:literal) => {
        BootstrapSource {
            key: $key,
            constant: $constant,
            marker: $marker,
            source: include_str!($file),
        }
    };
}

const BOOTSTRAP_SOURCES: &[BootstrapSource] = &[
    source!(
        "export_verifier",
        "EXPORT_VERIFIER",
        "EXPORT_VERIFIER",
        "release_export_contract.py"
    ),
    source!(
        "bootstrap",
        "BOOTSTRAP",
        "BOOTSTRAP",
        "release_generator_bootstrap.sh"
    ),
    source!("manager", "MANAGER", "MANAGER", "release_manager_bus.py"),
    source!(
        "watch_admission",
        "WATCH_ADMISSION",
        "WATCH_ADMISSION",
        "release_watch_admission.py"
    ),
    source!(
        "manager_policy_host",
        "MANAGER_POLICY_HOST",
        "MANAGER_POLICY_HOST",
        "release_manager_policy_host.py"
    ),
    source!(
        "manager_policy_files",
        "MANAGER_POLICY_FILES",
        "MANAGER_POLICY_FILES",
        "release_manager_policy_files.py"
    ),
    source!(
        "manager_policy",
        "MANAGER_POLICY",
        "MANAGER_POLICY",
        "release_manager_policy.py"
    ),
    source!(
        "lifecycle",
        "LIFECYCLE",
        "LIFECYCLE",
        "release_manager_lifecycle.py"
    ),
    source!("helper", "HELPER", "HELPER", "release_supervisor_exec.py"),
    source!(
        "bootstrap_watch",
        "WATCH_BOOTSTRAP",
        "WATCH_BOOTSTRAP",
        "release_bootstrap_watch.py"
    ),
    source!(
        "watch_program",
        "WATCH_PROGRAM",
        "WATCH_PROGRAM",
        "release_launcher_watch.py"
    ),
    source!(
        "claim",
        "CLAIM",
        "WATCH_CLAIM",
        "release_supervisor_claim.py"
    ),
];

pub(crate) fn bootstrap_sources() -> &'static [BootstrapSource] {
    BOOTSTRAP_SOURCES
}

fn supervisor_sources() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("coordinator", include_str!("release_coordinator_exec.py")),
        ("retained", include_str!("release_retained_parent.py")),
        ("source", include_str!("release_source_executor.py")),
        ("channel", include_str!("release_source_channel.py")),
        ("upload", include_str!("release_upload_operation.py")),
        ("broker", include_str!("release_forge_broker.py")),
    ])
}

pub(crate) fn supervisor_bundle() -> serde_json::Value {
    serde_json::json!(
        supervisor_sources()
            .into_iter()
            .map(|(key, source)| (key, STANDARD.encode(source.as_bytes())))
            .collect::<BTreeMap<_, _>>()
    )
}

pub(crate) fn export_contract() -> serde_json::Value {
    serde_json::json!({
        "schema": 1,
        "bootstrap": bootstrap_sources().iter().map(|source| serde_json::json!({
            "key": source.key, "constant": source.constant, "marker": source.marker,
            "sha256": digest(source.source.as_bytes()),
        })).collect::<Vec<_>>(),
        "supervisor": supervisor_sources().into_iter()
            .map(|(key, source)| (key, digest(source.as_bytes())))
            .collect::<BTreeMap<_, _>>(),
    })
}

fn digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .into_iter()
        .flat_map(|byte| [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]])
        .map(char::from)
        .collect()
}
