// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;

pub(crate) struct BootstrapSource {
    pub constant: &'static str,
    pub source: &'static str,
}

macro_rules! source {
    ($constant:literal, $file:literal) => {
        BootstrapSource {
            constant: $constant,
            source: include_str!($file),
        }
    };
}

const BOOTSTRAP_SOURCES: &[BootstrapSource] = &[
    source!("EXPORT_VERIFIER", "release_export_contract.py"),
    source!("BOOTSTRAP", "release_generator_bootstrap.sh"),
    source!("MANAGER", "release_manager_bus.py"),
    source!("WATCH_ADMISSION", "release_watch_admission.py"),
    source!("MANAGER_POLICY_HOST", "release_manager_policy_host.py"),
    source!("MANAGER_POLICY_FILES", "release_manager_policy_files.py"),
    source!("MANAGER_POLICY", "release_manager_policy.py"),
    source!("LIFECYCLE", "release_manager_lifecycle.py"),
    source!("HELPER", "release_supervisor_exec.py"),
    source!("WATCH_BOOTSTRAP", "release_bootstrap_watch.py"),
    source!("WATCH_PROGRAM", "release_launcher_watch.py"),
    source!("CLAIM", "release_supervisor_claim.py"),
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
        "schema": 2,
        "bootstrap": bootstrap_sources().iter().map(|source| serde_json::json!({
            "constant": source.constant,
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
