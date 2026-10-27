// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use sha2::{Digest, Sha256};
use velnor_actions_contract::{ReleaseManifest, StepKind, StepRole, TargetRecord};
use velnor_actions_workflow_renderer::closure::{
    BootstrapControllerPin, HelperProvenance, SEED_REMEDIATION, provision_acquire_step,
    provision_bootstrap_acquire_steps,
};
use velnor_actions_workflow_renderer::steps::{
    ASSET_SHA_ENV, ASSET_URL_ENV, BOOTSTRAP_EXPECTED_KEY_ENV, BOOTSTRAP_EXPECTED_SHA_ENV,
    BOOTSTRAP_MARKER_SUFFIX, BOOTSTRAP_MATCHED_KEY_ENV, BOOTSTRAP_MAX_BYTES,
    BOOTSTRAP_MAX_BYTES_ENV, BOOTSTRAP_MEMBER_ENV, BOOTSTRAP_SHADOW_CLEAR, BOOTSTRAP_SHADOW_ENV,
    BOOTSTRAP_STEP_NAME, BootstrapBridge, CACHE_BOOTSTRAP_SCRIPT, RELEASE_COMMIT_ENV,
    STAGED_BINARY_PREFIX, bootstrap_bridge_script,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

fn forbid(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Err(fail(&format!("{label} kept:{needle}")))
    } else {
        Ok(())
    }
}

fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

fn commit() -> String {
    "0123456789abcdef0123456789abcdef01234567".to_owned()
}

fn artifact_for(ver: &str, triple: &str) -> String {
    format!(
        "https://github.com/tailrocks/velnor-new/releases/download/v{ver}/velnor-actions-{ver}-{triple}"
    )
}

fn artifact(triple: &str) -> String {
    artifact_for(&version(), triple)
}

fn manifest_for(ver: &str) -> ReleaseManifest {
    ReleaseManifest {
        schema: 1,
        version: ver.to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        commit: commit(),
        targets: vec![
            TargetRecord {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: artifact_for(ver, "x86_64-unknown-linux-gnu"),
                sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_owned(),
            },
            TargetRecord {
                target: "aarch64-apple-darwin".to_owned(),
                artifact: artifact_for(ver, "aarch64-apple-darwin"),
                sha256: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                    .to_owned(),
            },
            TargetRecord {
                target: "x86_64-apple-darwin".to_owned(),
                artifact: artifact_for(ver, "x86_64-apple-darwin"),
                sha256: "789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456"
                    .to_owned(),
            },
        ],
    }
}

fn manifest() -> ReleaseManifest {
    manifest_for(&version())
}

fn pin() -> Result<BootstrapControllerPin, Box<dyn Error>> {
    Ok(BootstrapControllerPin::from_release_manifest(
        &manifest(),
        "ubuntu-24.04",
        &version(),
    )?)
}

fn provenance(pin: &BootstrapControllerPin) -> HelperProvenance {
    HelperProvenance::ReleaseAsset {
        url: pin.url.clone(),
        sha256: pin.sha256.clone(),
        commit: pin.commit.clone(),
    }
}

fn staged() -> String {
    format!("{STAGED_BINARY_PREFIX}{}", version())
}

fn acquire_argv(staged: &str) -> Vec<String> {
    let script = format!(
        "d=\"{staged}\"&&mkdir -p \"${{d%/*}}\"&&p=\"$VELNOR_ASSET_SHA256  \"&&curl -fsSL --connect-timeout 15 --max-time 120 --retry 5 --retry-all-errors --retry-max-time 180 --proto '=https' --tlsv1.2 \"$VELNOR_ASSET_URL\" -o \"$d\"&&echo \"$p$d\"|sha256sum -c -&&chmod +x \"$d\""
    );
    vec!["sh".to_owned(), "-c".to_owned(), script]
}

fn bridge(payload_dir: &str, staged: &str) -> BootstrapBridge {
    BootstrapBridge {
        payload_dir: payload_dir.to_owned(),
        member: format!("controller/velnor-actions-{}", version()),
        staged_path: staged.to_owned(),
        max_bytes: BOOTSTRAP_MAX_BYTES,
        expected_key: None,
        matched_key: None,
        shadow_state: None,
    }
}

type ShellView<'a> = (&'a Vec<String>, &'a BTreeMap<String, String>);

fn shell_of(step: &velnor_actions_contract::Step) -> Result<ShellView<'_>, Box<dyn Error>> {
    match &step.kind {
        StepKind::Shell { run, env } => Ok((run, env)),
        _ => Err(fail("expected a shell step")),
    }
}

#[path = "gc1_t43_composer.rs"]
mod gc1_t43_composer;
#[path = "gc1_t43_handoff.rs"]
mod gc1_t43_handoff;
#[path = "gc1_t43_reject.rs"]
mod gc1_t43_reject;
#[path = "gc1_t43_warm.rs"]
mod gc1_t43_warm;
pub(crate) use gc1_t43_composer::*;
pub(crate) use gc1_t43_warm::*;
