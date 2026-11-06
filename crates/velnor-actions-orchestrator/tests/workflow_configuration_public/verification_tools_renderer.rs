// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::{
    BuildTask, BuildTaskRunner, MiseTaskSource, VerificationRunner, VerificationTask,
};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::{
    MiseSetup,
    setup::MISE_BINARY_SHA256_MACOS_ARM64,
    verification_jobs::{
        BuildTaskArtifact, BuildTaskPolicy, BuildTaskTool, VerificationTaskPolicy,
        build_build_task_job, build_verification_task_job,
    },
};

const NEXT: &str = "aqua:nextest-rs/nextest/cargo-nextest";

#[test]
fn public_build_task_renderer_rejects_unapproved_checksum_source() -> Result<(), Box<dyn Error>> {
    let error = build_build_task_job(
        &policy(),
        "actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .err()
    .ok_or("unapproved checksum source was accepted")?;
    assert!(
        error
            .to_string()
            .contains("build_task_artifact:aqua:nextest-rs/nextest/cargo-nextest"),
        "{error}"
    );
    Ok(())
}

#[test]
fn public_verification_task_renderer_rejects_unapproved_checksum_source()
-> Result<(), Box<dyn Error>> {
    let error = build_verification_task_job(
        &verification_policy(),
        "actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .err()
    .ok_or("VerificationTask renderer accepted an unapproved checksum source")?;
    assert!(
        error
            .to_string()
            .contains("build_task_artifact:aqua:nextest-rs/nextest/cargo-nextest"),
        "{error}"
    );
    Ok(())
}

fn policy() -> BuildTaskPolicy {
    BuildTaskPolicy {
        task: BuildTask {
            id: "mise-v4-tools".to_owned(),
            mise_task: "verify:hook-closure".to_owned(),
            source: MiseTaskSource::default(),
            tools: vec![
                NEXT.to_owned(),
                "mr-boxington".to_owned(),
                "rust".to_owned(),
            ],
            runner: BuildTaskRunner::Macos26Arm64,
            timeout_minutes: 10,
            cargo_build_jobs: 1,
            nextest_test_threads: 1,
            artifacts: Vec::new(),
        },
        runner_label: "macos-26".to_owned(),
        mise_setup: MiseSetup {
            uses: "jdx/mise-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            version: MISE_VERSION.to_owned(),
            sha256: MISE_BINARY_SHA256_MACOS_ARM64.to_owned(),
        },
        mise_config_sha256: "a".repeat(64),
        mise_lock_sha256: "b".repeat(64),
        rust_toolchain_sha256: "c".repeat(64),
        source_mise_config_sha256: "d".repeat(64),
        source_mise_lock_sha256: None,
        source_rust_toolchain_sha256: None,
        selected_tools: selected_tools(),
    }
}

fn verification_policy() -> VerificationTaskPolicy {
    VerificationTaskPolicy {
        task: VerificationTask {
            id: "mise-v4-tools".to_owned(),
            mise_task: "verify:hook-closure".to_owned(),
            source: MiseTaskSource::default(),
            runner: VerificationRunner::LinuxX64,
            timeout_minutes: 10,
        },
        runner_label: "ubuntu-26.04".to_owned(),
        scale_set_token: None,
        mise_setup: MiseSetup {
            uses: "jdx/mise-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            version: MISE_VERSION.to_owned(),
            sha256: "e".repeat(64),
        },
        selected_tools: selected_tools(),
        mise_config_sha256: Some("a".repeat(64)),
        mise_lock_sha256: Some("b".repeat(64)),
        rust_toolchain_sha256: Some("c".repeat(64)),
    }
}

fn selected_tools() -> Vec<BuildTaskTool> {
    vec![
        BuildTaskTool {
            key: NEXT.to_owned(),
            version: "0.9.151".to_owned(),
            backend: NEXT.to_owned(),
            os: Vec::new(),
            config_options: BTreeMap::new(),
            lock_options: BTreeMap::new(),
            artifact: Some(artifact(
                'a',
                Some("untrusted-source"),
                "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.151/cargo-nextest.tar.gz",
            )),
        },
        BuildTaskTool {
            key: "mr-boxington".to_owned(),
            version: "1.29.2".to_owned(),
            backend: "packslip:github.com/jdx/mr-boxington".to_owned(),
            os: Vec::new(),
            config_options: BTreeMap::new(),
            lock_options: BTreeMap::new(),
            artifact: Some(artifact(
                'b',
                None,
                "https://github.com/jdx/mr-boxington/releases/download/v1.29.2/mbx-aarch64-apple-darwin.tar.gz",
            )),
        },
        BuildTaskTool {
            key: "rust".to_owned(),
            version: "1.99.1".to_owned(),
            backend: "core:rust".to_owned(),
            os: Vec::new(),
            config_options: BTreeMap::from([("profile".to_owned(), "minimal".to_owned())]),
            lock_options: BTreeMap::from([("profile".to_owned(), "minimal".to_owned())]),
            artifact: None,
        },
    ]
}

fn artifact(checksum_byte: char, checksum_source: Option<&str>, url: &str) -> BuildTaskArtifact {
    BuildTaskArtifact {
        checksum: format!("sha256:{}", checksum_byte.to_string().repeat(64)),
        checksum_source: checksum_source.map(ToOwned::to_owned),
        url: url.to_owned(),
        url_api: None,
        signer: None,
        provenance: None,
        provenance_verified: None,
        repository_ids: None,
    }
}
