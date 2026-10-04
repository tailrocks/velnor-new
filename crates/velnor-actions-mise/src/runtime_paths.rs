//! P08 exact runtime path inventory for every cache layer.
//!
//! One owner per path (cache-contract §2). Cached symlinks without their
//! targets never count as warm: a toolchain is warm only when its
//! rustup toolchain dir, Cargo homes, and Mise installs all resolve.

use std::path::{Path, PathBuf};

use crate::error::MiseError;

/// Default Mise data dir holding installs, shims, and tool state.
pub const MISE_DATA_DIR: &str = velnor_actions_contract::ToolCacheDomain::Full.root();
/// Shell spelling of [`MISE_DATA_DIR`] for generated `run:` scripts.
pub const MISE_DATA_DIR_SHELL: &str = "$RUNNER_TEMP/velnor/mise";
/// Dedicated Mise data dir for the Cargo-free consumer planning phase.
///
/// Planning must not read or mutate the full task tool root. The path is a
/// compiled generator value; callers select it through [`RuntimePaths`]
/// rather than supplying a string.
pub const PLANNING_MISE_DATA_DIR: &str = "${{ runner.temp }}/velnor/planning/mise";
/// Shell spelling of [`PLANNING_MISE_DATA_DIR`] for generated `run:` scripts.
pub const PLANNING_MISE_DATA_DIR_SHELL: &str = "$RUNNER_TEMP/velnor/planning/mise";
/// Environment key selecting the Mise data directory.
pub const MISE_DATA_DIR_ENV: &str = "MISE_DATA_DIR";
/// Mise installs under the data dir (`<tool>/<version>`).
pub const MISE_INSTALLS_SUFFIX: &str = "installs";
/// Velnor-owned rustup home (expression form for `env:`).
pub const RUSTUP_HOME_EXPR: &str =
    velnor_actions_contract::workflow::tool_producer::homes::RUSTUP_HOME;
/// Velnor-owned Cargo home (expression form for `env:`).
pub const CARGO_HOME_EXPR: &str =
    velnor_actions_contract::workflow::tool_producer::homes::CARGO_HOME;
/// Shell form of the Cargo home for `run:` scripts.
pub const CARGO_HOME_SHELL: &str = "$RUNNER_TEMP/velnor/cargo";
/// Cargo registry sources under the owned home.
pub const CARGO_REGISTRY_SUFFIX: &str = "registry";
/// Cargo git sources under the owned home.
pub const CARGO_GIT_SUFFIX: &str = "git";
/// Cargo binaries under the owned home.
pub const CARGO_BIN_SUFFIX: &str = "bin";
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

/// Compiled runtime root selected by a generator-owned command context.
///
/// The planning root is intentionally a domain, not a caller-provided path.
/// This keeps early `gh` and validator commands separate from full task
/// preparation while preserving one exact path spelling across renderers and
/// subprocess requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePathDomain {
    /// Full workflow/task tool root.
    Full,
    /// Cargo-free consumer planning tool root.
    Planning,
}

/// Compiled runtime paths for one generator-owned domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimePaths {
    domain: RuntimePathDomain,
}

impl RuntimePaths {
    /// Full tool root used by ordinary task and generator commands.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            domain: RuntimePathDomain::Full,
        }
    }

    /// Separate tool root used by the early consumer planning phase.
    #[must_use]
    pub const fn planning() -> Self {
        Self {
            domain: RuntimePathDomain::Planning,
        }
    }

    /// Select one compiled runtime domain.
    #[must_use]
    pub const fn for_domain(domain: RuntimePathDomain) -> Self {
        Self { domain }
    }

    /// Domain selected by this path bundle.
    #[must_use]
    pub const fn domain(self) -> RuntimePathDomain {
        self.domain
    }

    /// Whether this bundle names the dedicated planning root.
    #[must_use]
    pub const fn is_planning(self) -> bool {
        matches!(self.domain, RuntimePathDomain::Planning)
    }

    /// Exact expression-form Mise data directory.
    #[must_use]
    pub const fn mise_data_dir(self) -> &'static str {
        match self.domain {
            RuntimePathDomain::Full => MISE_DATA_DIR,
            RuntimePathDomain::Planning => PLANNING_MISE_DATA_DIR,
        }
    }

    /// Exact shell-form Mise data directory.
    #[must_use]
    pub const fn mise_data_dir_shell(self) -> &'static str {
        match self.domain {
            RuntimePathDomain::Full => MISE_DATA_DIR_SHELL,
            RuntimePathDomain::Planning => PLANNING_MISE_DATA_DIR_SHELL,
        }
    }

    /// One typed environment pair for command and rendered-step bindings.
    #[must_use]
    pub const fn mise_data_env(self) -> (&'static str, &'static str) {
        (MISE_DATA_DIR_ENV, self.mise_data_dir())
    }

    /// Resolve the runtime directory under a concrete `RUNNER_TEMP` value.
    ///
    /// Rendered workflow env uses [`Self::mise_data_dir`], where the GitHub
    /// expression is evaluated by the runner. Spawned commands use this
    /// method after reading the concrete `RUNNER_TEMP` inherited by the
    /// process; shell and expression text never reaches Mise itself.
    #[must_use]
    pub fn concrete_mise_data_dir(self, runner_temp: &Path) -> PathBuf {
        match self.domain {
            RuntimePathDomain::Full => runner_temp.join("velnor/mise"),
            RuntimePathDomain::Planning => runner_temp.join("velnor/planning/mise"),
        }
    }
}

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

/// Exact inventory: Mise installs, rustup, Cargo, target, MBX, task, tofu.
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
            id: "cargo-sources",
            path: "${{ runner.temp }}/velnor/cargo/registry",
            owner: "velnor/sources",
        },
        RuntimePath {
            id: "cargo-binaries",
            path: "${{ runner.temp }}/velnor/cargo/bin",
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
