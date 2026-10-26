// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::MiseError;

pub const MISE_DATA_DIR: &str = "~/.local/share/mise";
pub const MISE_INSTALLS_SUFFIX: &str = "installs";
pub const RUSTUP_HOME_EXPR: &str = "${{ runner.temp }}/velnor/rustup";
pub const CARGO_HOME_EXPR: &str = "${{ runner.temp }}/velnor/cargo";
pub const CARGO_HOME_SHELL: &str = "$RUNNER_TEMP/velnor/cargo";
pub const CARGO_REGISTRY_SUFFIX: &str = "registry";
pub const CARGO_GIT_SUFFIX: &str = "git";
pub const CARGO_BIN_PATH_EXPR: &str = "${{ runner.temp }}/velnor/cargo/bin";
pub const CARGO_INSTALL_RECEIPT_PATH_EXPR: &str = "${{ runner.temp }}/velnor/cargo/.crates.toml";
pub const CARGO_INSTALL_RECEIPT_JSON_PATH_EXPR: &str =
    "${{ runner.temp }}/velnor/cargo/.crates2.json";
pub const TARGET_BASE: &str = "$RUNNER_TEMP/velnor/target/";
pub const TOFU_DATA_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-data";
pub const TOFU_PROVIDER_CACHE_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-cache";
pub const MBX_OBJECTS_OWNER: &str = "mr-boxington-action/objects";
pub const TASK_ARTIFACTS_SUFFIX: &str = "task-artifacts/v2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePath {
    pub id: &'static str,
    pub path: &'static str,
    pub owner: &'static str,
}

#[must_use]
pub fn inventory() -> Vec<RuntimePath> {
    vec![
        RuntimePath {
            id: "mise-installs",
            path: MISE_DATA_DIR,
            owner: "catalog/tools",
        },
        RuntimePath {
            id: "tofu-provider-cache",
            path: TOFU_PROVIDER_CACHE_BASE_EXPR,
            owner: "velnor/tofu-providers",
        },
        RuntimePath {
            id: "rustup-toolchains",
            path: RUSTUP_HOME_EXPR,
            owner: "catalog/tools",
        },
        RuntimePath {
            id: "cargo-registry-sources",
            path: "${{ runner.temp }}/velnor/cargo/registry",
            owner: "velnor/sources",
        },
        RuntimePath {
            id: "cargo-git-sources",
            path: "${{ runner.temp }}/velnor/cargo/git",
            owner: "velnor/sources",
        },
        RuntimePath {
            id: "cargo-install-binaries",
            path: CARGO_BIN_PATH_EXPR,
            owner: "catalog/tools",
        },
        RuntimePath {
            id: "cargo-install-receipt",
            path: CARGO_INSTALL_RECEIPT_PATH_EXPR,
            owner: "catalog/tools",
        },
        RuntimePath {
            id: "cargo-install-receipt-json",
            path: CARGO_INSTALL_RECEIPT_JSON_PATH_EXPR,
            owner: "catalog/tools",
        },
        RuntimePath {
            id: "cargo-target",
            path: TARGET_BASE,
            owner: "job/target",
        },
        RuntimePath {
            id: "mbx-objects",
            path: MBX_OBJECTS_OWNER,
            owner: "mr-boxington/MBX",
        },
        RuntimePath {
            id: "mise-task-artifacts",
            path: TASK_ARTIFACTS_SUFFIX,
            owner: "mise/task-result",
        },
    ]
}

#[must_use]
pub fn is_known_id(id: &str) -> bool {
    inventory().iter().any(|entry| entry.id == id)
}

#[must_use]
pub fn is_warm_toolchain(rustup_dir: bool, cargo_dir: bool, shim_target: Option<bool>) -> bool {
    rustup_dir && cargo_dir && shim_target.is_some_and(|ok| ok)
}

pub fn validate_no_credentials(path: &str) -> Result<(), MiseError> {
    let lower = path.to_ascii_lowercase();
    if lower.contains("credentials") || lower.contains("config.toml") && lower.contains("token") {
        return Err(MiseError::InvalidStepInput {
            field: "cache_path".to_owned(),
            value: path.to_owned(),
        });
    }
    Ok(())
}

pub fn owner_for(id: &str) -> Result<&'static str, MiseError> {
    inventory()
        .into_iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.owner)
        .ok_or_else(|| MiseError::InvalidStepInput {
            field: "runtime_path".to_owned(),
            value: id.to_owned(),
        })
}
