// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const PREVIEW_PRODUCER_RECIPE_KEY: &str = "preview.producer.task";

pub(super) const FAMILY_BUILD: &str = "build";
pub(super) const FAMILY_TEST: &str = "test";
pub(super) const FAMILY_CLIPPY: &str = "clippy";
pub(super) const FAMILY_DOC: &str = "doc";
pub(super) const FAMILY_NAMED_CHECK: &str = "named-check";
pub(super) const FAMILY_HELPER: &str = "helper";
pub(super) const FAMILY_NATIVE_PRODUCER: &str = "native-producer";
pub(super) const FAMILY_PREVIEW_PRODUCER: &str = "preview-producer";

pub(super) const ROUTE_MBX_BUILD: &str = "mise-exec[rust,mr-boxington]:mbx";
pub(super) const ROUTE_MBX_NEXTEST: &str = "mise-exec[rust,mr-boxington,nextest]:mbx";

pub(super) const GNU_LINUX: &str = "x86_64-unknown-linux-gnu";
pub(super) const MUSL_LINUX: &str = "x86_64-unknown-linux-musl";
pub(super) const MACOS_X86_64: &str = "x86_64-apple-darwin";

pub(super) const PROFILE_RELEASE: &str = "release";
pub(super) const NEXTEST_DEFAULT: &str = "default";

pub(super) const PACKAGE_CLI: &str = "velnor-actions-cli";
pub(super) const PACKAGE_RUNNER_CLI: &str = "velnor-runner-cli";
pub(super) const PACKAGE_RESOURCE_PROBE: &str = "velnor-resource-probe";
pub(super) const PACKAGE_RUNNER_HOST: &str = "velnor-runner-host";

pub(super) const BUILDER_MBX_BUILD_ARGV: &str = "product_release_pins::mbx_build_argv";
pub(super) const BUILDER_RUNNER_BUILD_ARGV: &str = "product_release_pins::runner_build_argv";
pub(super) const BUILDER_RESOURCE_PROBE_BUILD_ARGV: &str =
    "product_release_pins::resource_probe_build_argv";
pub(super) const BUILDER_RESOURCE_PROBE_RELEASE_BUILD_ARGV: &str =
    "product_release_pins::resource_probe_release_build_argv";
pub(super) const BUILDER_RESOURCE_PROBE_RELEASE_TEST_ARGV: &str =
    "product_release_pins::resource_probe_release_test_argv";
pub(super) const BUILDER_RELEASE_ASSET_BUILD_STEPS: &str =
    "workflow_renderer::schema2_generator_release_assets::build_steps";
pub(super) const BUILDER_NATIVE_IMAGE_PRODUCER: &str =
    "workflow_renderer::schema2_job_image_native";
pub(super) const BUILDER_PRESEED_LOCAL_BUILD: &str = "workflow_renderer::preseed::PRESEED_BUILD_OUTPUT";
pub(super) const BUILDER_PREVIEW_PRODUCER_TASK: &str = "preview_producer::execute_preview_task";

pub(super) const MBX_CLI_BUILD_ARGV: &[&str] = &[
    "build",
    "--release",
    "--locked",
    "--package",
    "velnor-actions-cli",
    "--bin",
    "velnor-actions",
];
pub(super) const MBX_CLI_INTEL_BUILD_ARGV: &[&str] = &[
    "build",
    "--release",
    "--locked",
    "--package",
    "velnor-actions-cli",
    "--bin",
    "velnor-actions",
    "--target",
    "x86_64-apple-darwin",
];
pub(super) const RUNNER_HOST_BUILD_ARGV: &[&str] = &[
    "build",
    "--locked",
    "--manifest-path",
    "crates/velnor-runner/Cargo.toml",
    "--release",
    "--package",
    "velnor-runner-cli",
    "--bin",
    "velnor-host",
];
pub(super) const RESOURCE_PROBE_MUSL_BUILD_ARGV: &[&str] = &[
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
];
pub(super) const RESOURCE_PROBE_RELEASE_GNU_BUILD_ARGV: &[&str] = &[
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
];
pub(super) const RESOURCE_PROBE_RELEASE_NEXTEST_ARGV: &[&str] = &[
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
    "--no-tests",
    "fail",
];

pub(super) const WORKSPACE_SOURCE_INPUTS: &[&str] = &[
    "Cargo.lock",
    "Cargo.toml",
    "crates/velnor-actions-cli/Cargo.toml",
];
pub(super) const RUNNER_SOURCE_INPUTS: &[&str] = &[
    "Cargo.lock",
    "Cargo.toml",
    "crates/velnor-runner/Cargo.toml",
];
pub(super) const NATIVE_IMAGE_SOURCE_INPUTS: &[&str] = &[
    ".git",
    "Cargo.lock",
    "Cargo.toml",
    "crates/velnor-actions-cli/Cargo.toml",
];
pub(super) const PREVIEW_SOURCE_BINDINGS: &[&str] = &[
    "preview.producer.inputs",
    "preview.producer.output_directory",
];
pub(super) const STAGED_HELPER_INPUTS: &[&str] = &["target/release/velnor-actions"];

pub(super) const PREVIEW_HANDOFF_ENV: &[&str] = &[
    "CARGO_BUILD_JOBS",
    "VELNOR_RELEASE_COMMIT_COUNT",
    "VELNOR_RELEASE_OUTPUT_DIRECTORY",
    "VELNOR_RELEASE_OUTPUT_MANIFEST",
    "VELNOR_RELEASE_SOURCE_COMMIT",
    "VELNOR_RELEASE_SOURCE_REF",
    "VELNOR_RELEASE_TASK_REQUEST_FILE",
    "VELNOR_RELEASE_VERSION",
];
pub(super) const NATIVE_IMAGE_ENV: &[&str] = &["VELNOR_NATIVE_IMAGE_ACTIONS"];

pub(super) const HOST_TARGET_DIR_ROLE: &[&str] = &["target/release"];
pub(super) const INTEL_TARGET_DIR_ROLE: &[&str] = &["target/x86_64-apple-darwin/release"];
pub(super) const HELPER_BINARY_ROLE: &[&str] = &["target/release/velnor-actions"];
pub(super) const PREVIEW_OUTPUT_ROLES: &[&str] = &["dist/preview", "dist/preview/package-output.json"];
pub(super) const STAGED_IMAGE_BINARY_ROLE: &[&str] = &["VELNOR_NATIVE_IMAGE_ACTIONS"];
