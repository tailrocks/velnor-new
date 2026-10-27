// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
pub(crate) fn argv(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

pub(crate) fn verify_rust_argv() -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        &format!("{} exit 0", credential_unset_prelude()),
    ])
}

pub(crate) fn verify_lint_argv() -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        &jactionlint_verifier_script("${VELNOR_TOOL_JACTIONLINT}", VERIFY_PATH),
    ])
}

pub(crate) fn install_runner_build_tools_argv() -> Vec<String> {
    argv(&["mise", "install", "rust@1.91.0"])
}

pub(crate) fn install_resource_probe_target_argv() -> Vec<String> {
    argv(&["rustup", "target", "add", "x86_64-unknown-linux-musl"])
}

pub(crate) fn runner_build_argv() -> Vec<String> {
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

pub(crate) fn attestation_helper_build_argv() -> Vec<String> {
    argv(&[
        "mbx",
        "build",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--release",
        "--package",
        "velnor-runner-attestation",
        "--bin",
        "velnor-runner-attestation-helper",
    ])
}

pub(crate) fn resource_probe_build_argv() -> Vec<String> {
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

pub(crate) fn install_resource_probe_release_tools_argv() -> Vec<String> {
    argv(&["mise", "install", "rust@1.91.0", "mr-boxington@1.0.0"])
}

pub(crate) fn resource_probe_release_build_argv() -> Vec<String> {
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

pub(crate) fn resource_probe_release_test_argv() -> Vec<String> {
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
