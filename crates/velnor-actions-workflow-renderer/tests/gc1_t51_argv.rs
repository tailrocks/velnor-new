// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::jactionlint_verifier::jactionlint_verifier_script;
use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::VERIFY_PATH;
use velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude;

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

pub(super) fn tar_entry(name: &str, data: &[u8], typeflag: u8) -> Vec<u8> {
    let mut header = [0u8; 512];
    let name_bytes = name.as_bytes();
    header[..name_bytes.len()].copy_from_slice(name_bytes);
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    let size_field = format!("{:011o}\0", data.len());
    header[124..136].copy_from_slice(size_field.as_bytes());
    header[148..156].copy_from_slice(b"        ");
    header[156] = typeflag;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    let checksum_field = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(checksum_field.as_bytes());
    let mut entry = header.to_vec();
    entry.extend_from_slice(data);
    while entry.len() % 512 != 0 {
        entry.push(0);
    }
    entry
}

pub(super) fn tar_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = Vec::new();
    for (name, data) in entries {
        archive.extend_from_slice(&tar_entry(name, data, b'0'));
    }
    archive.extend_from_slice(&[0u8; 1024]);
    archive
}
pub(super) fn argv(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

pub(super) fn fixture_step() -> Step {
    Step {
        name: "fixture".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

pub(super) fn verify_rust_argv() -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        &format!("{} exit 0", credential_unset_prelude()),
    ])
}

pub(super) fn verify_lint_argv() -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        &jactionlint_verifier_script("${VELNOR_TOOL_JACTIONLINT}", VERIFY_PATH),
    ])
}

pub(super) fn install_runner_build_tools_argv() -> Vec<String> {
    argv(&["mise", "install", "rust@1.91.0"])
}

pub(super) fn install_resource_probe_target_argv() -> Vec<String> {
    argv(&["rustup", "target", "add", "x86_64-unknown-linux-musl"])
}

pub(super) fn runner_build_argv() -> Vec<String> {
    argv(&[
        "mbx",
        "build",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--release",
        "--package",
        "velnor-runner-cli",
        "--bin",
        "velnor-host",
    ])
}

pub(super) fn resource_probe_build_argv() -> Vec<String> {
    argv(&[
        "mbx",
        "build",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--package",
        "velnor-resource-probe",
        "--bin",
        "velnor-resource-probe",
        "--release",
        "--target",
        "x86_64-unknown-linux-musl",
    ])
}

pub(super) fn install_resource_probe_release_tools_argv() -> Vec<String> {
    argv(&["mise", "install", "rust@1.91.0", "mr-boxington@1.0.0"])
}

pub(super) fn resource_probe_release_build_argv() -> Vec<String> {
    argv(&[
        "mbx",
        "build",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--package",
        "velnor-runner-host",
        "--bin",
        "velnor-resource-probe-release",
        "--target",
        "x86_64-unknown-linux-gnu",
    ])
}

pub(super) fn resource_probe_release_test_argv() -> Vec<String> {
    argv(&[
        "mbx",
        "nextest",
        "run",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--package",
        "velnor-runner-host",
        "--test",
        "resource_probe_release_public",
        "--target",
        "x86_64-unknown-linux-gnu",
    ])
}
