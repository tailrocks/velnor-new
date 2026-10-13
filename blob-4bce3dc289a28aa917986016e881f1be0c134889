//! Exact runtime path inventory for every cache layer.
//!
//! One owner per path (cache-contract §2). Cached symlinks without their
//! targets never count as warm: a toolchain is warm only when its
//! rustup toolchain dir, Cargo homes, and Mise installs all resolve.

use crate::error::MiseError;

/// Default Mise data dir holding installs, shims, and tool state.
pub const MISE_DATA_DIR: &str = "~/.local/share/mise";
/// Mise installs under the data dir (`<tool>/<version>`).
pub const MISE_INSTALLS_SUFFIX: &str = "installs";
/// Velnor-owned rustup home (expression form for `env:`).
pub const RUSTUP_HOME_EXPR: &str = "${{ runner.temp }}/velnor/rustup";
/// Velnor-owned Cargo home (expression form for `env:`).
pub const CARGO_HOME_EXPR: &str = "${{ runner.temp }}/velnor/cargo";
/// Shell form of the Cargo home for `run:` scripts.
pub const CARGO_HOME_SHELL: &str = "$RUNNER_TEMP/velnor/cargo";
/// Cargo registry sources under the owned home.
pub const CARGO_REGISTRY_SUFFIX: &str = "registry";
/// Cargo git sources under the owned home.
pub const CARGO_GIT_SUFFIX: &str = "git";
/// Cargo binaries under the owned home.
pub const CARGO_BIN_PATH_EXPR: &str = "${{ runner.temp }}/velnor/cargo/bin";
/// Cargo install receipt for tool binaries.
pub const CARGO_INSTALL_RECEIPT_PATH_EXPR: &str = "${{ runner.temp }}/velnor/cargo/.crates.toml";
/// Cargo install receipt for tool binaries (JSON format).
pub const CARGO_INSTALL_RECEIPT_JSON_PATH_EXPR: &str =
    "${{ runner.temp }}/velnor/cargo/.crates2.json";
/// Per-lane target base (concurrent writers never share).
pub const TARGET_BASE: &str = "$RUNNER_TEMP/velnor/target/";
/// Velnor-owned tofu data-dir base (expression form for `env:`).
pub const TOFU_DATA_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-data";
/// Velnor-owned tofu provider-cache base (expression form for `env:`).
///
/// Shell spelling `$RUNNER_TEMP/velnor/tofu-cache/<slug>`; per-root
/// slugs hang under this base, one job-private dir per root.
pub const TOFU_PROVIDER_CACHE_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-cache";
/// MBX objects are action-managed, never a filesystem archive path.
pub const MBX_OBJECTS_OWNER: &str = "mr-boxington-action/objects";
/// Mise task artifacts dir (task-result layer).
pub const TASK_ARTIFACTS_SUFFIX: &str = "task-artifacts/v2";

/// One inventoried runtime path with its owning layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePath {
    /// Stable id (`mise-installs`, `rustup-toolchains`, ...).
    pub id: &'static str,
    /// Exact path or expression.
    pub path: &'static str,
    /// Owning layer (`catalog/tools`, `velnor/sources`, ...).
    pub owner: &'static str,
}

/// Exact inventory: Mise, Rustup, separate Cargo sources/tools, target, MBX, task, tofu.
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

/// True when `id` names a known inventory entry.
#[must_use]
pub fn is_known_id(id: &str) -> bool {
    inventory().iter().any(|entry| entry.id == id)
}

/// Warm toolchain check: symlinks without targets never count.
///
/// `rustup_dir` and `cargo_dir` must exist; `shim_target` is the
/// resolved target of the cached `rustc`/`cargo` shim (None when the
/// symlink dangles). Returns false for any missing input.
#[must_use]
pub fn is_warm_toolchain(rustup_dir: bool, cargo_dir: bool, shim_target: Option<bool>) -> bool {
    rustup_dir && cargo_dir && shim_target.is_some_and(|ok| ok)
}

/// Reject credential-bearing paths under Cargo home.
///
/// # Errors
///
/// Returns [`MiseError::InvalidStepInput`] for credentials, configs
/// carrying tokens, or paths outside the owned home.
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

/// Owner for one inventory id.
///
/// # Errors
///
/// Returns [`MiseError::InvalidStepInput`] for unknown ids.
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
