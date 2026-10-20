//! `bun-registry-install-v1`: the registry phase of `bun install` (HD-6, HD-7).
//!
//! With `private_fetch*`, the only code that reaches the private-fetch spawn
//! policy. The job's `GITHUB_TOKEN` (`packages: read`) arrives as
//! `VELNOR_NPM_READ_TOKEN`; Bun starts from a cleared environment with a
//! Velnor-written configuration and `--ignore-scripts`, so no repository
//! script runs while the token exists. The same operation writes the token-free
//! configuration the lifecycle phase reuses.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_bun::velnor_bunfig;
use velnor_actions_mise::command::{IsolatedCommand, PrivateFetchCredential};
use velnor_actions_mise::{MISE_GLOBAL_FLAGS, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::verification_jobs::{BUN_ROOT_ENV, BUN_SCOPES_ENV};

use crate::OrchestratorError;
use crate::config::private_fetch_session::Token;
use crate::internal::internal;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

/// Operation name (shared with the contract and the renderer).
pub const BUN_REGISTRY_INSTALL_OP: &str = velnor_actions_contract::BUN_REGISTRY_INSTALL_OPERATION;
/// Exact Bun version, exported by the project-tool resolver step.
const VERSION_ENV: &str = "VELNOR_TOOL_BUN";
const CACHE_ENV: &str = "BUN_INSTALL_CACHE_DIR";
/// Directory (under runner temp) holding both configuration files.
pub(crate) const CONFIG_SUBDIR: &str = "velnor/bun";
const INSTALL_CONFIG: &str = "registry.toml";
const LIFECYCLE_CONFIG: &str = "lifecycle.toml";
const INSTALL_TIMEOUT: Duration = Duration::from_mins(20);
const CAPTURE_BYTES: usize = 1024 * 1024;
const MISE_ENV_KEYS: [&str; 6] = [
    "MISE_NO_CONFIG",
    "MISE_NO_ENV",
    "MISE_NO_HOOKS",
    "MISE_LOCKFILE",
    "MISE_AUTO_INSTALL",
    "MISE_EXEC_AUTO_INSTALL",
];

/// Parsed and validated request.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct InstallRequest {
    pub(crate) root: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) version: String,
    pub(crate) workspace: PathBuf,
    pub(crate) runner_temp: PathBuf,
    pub(crate) cache_dir: PathBuf,
    pub(crate) mise_env: Vec<(String, String)>,
}

impl InstallRequest {
    /// Parse the step environment.
    ///
    /// # Errors
    /// Rejects missing, relative or malformed values.
    pub(crate) fn parse(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, OrchestratorError> {
        let need = |key: &str| {
            get(key)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| internal(&format!("bun_registry_missing_env:{key}")))
        };
        let absolute = |key: &str| -> Result<PathBuf, OrchestratorError> {
            let path = PathBuf::from(need(key)?);
            path.is_absolute()
                .then_some(path)
                .ok_or_else(|| internal(&format!("bun_registry_path_relative:{key}")))
        };
        let root = need(BUN_ROOT_ENV)?;
        let root = if root == "." { String::new() } else { root };
        velnor_actions_contract::validate_fetch_root(&root)
            .map_err(|_| internal("bun_registry_root_invalid"))?;
        let scopes: Vec<String> = need(BUN_SCOPES_ENV)?
            .split(',')
            .map(str::to_owned)
            .collect();
        let version = need(VERSION_ENV)?;
        if !velnor_actions_contract::config::is_exact_semver(&version) {
            return Err(internal("bun_registry_version_invalid"));
        }
        Ok(Self {
            root,
            scopes,
            version,
            workspace: absolute("GITHUB_WORKSPACE")?,
            runner_temp: absolute("RUNNER_TEMP")?,
            cache_dir: absolute(CACHE_ENV)?,
            mise_env: MISE_ENV_KEYS
                .iter()
                .filter_map(|key| get(key).map(|value| ((*key).to_owned(), value)))
                .collect(),
        })
    }

    fn config_dir(&self) -> PathBuf {
        self.runner_temp.join(CONFIG_SUBDIR)
    }

    fn work_dir(&self) -> PathBuf {
        if self.root.is_empty() {
            self.workspace.clone()
        } else {
            self.workspace.join(&self.root)
        }
    }
}

/// Write the registry and lifecycle configurations from the repository bunfig.
///
/// # Errors
/// Fails closed on `bunfig_registry_override`, an unreadable file or a write error.
pub(crate) fn write_configs(
    request: &InstallRequest,
) -> Result<(PathBuf, PathBuf), OrchestratorError> {
    let rel = if request.root.is_empty() {
        "bunfig.toml".to_owned()
    } else {
        format!("{}/bunfig.toml", request.root)
    };
    let repository = match read_repo_file(&request.workspace, &rel, MAX_REPO_FILE_BYTES)? {
        RepoRead::Text(text) => Some(text),
        RepoRead::Absent => None,
    };
    let render = |with_token: bool| {
        velnor_bunfig(repository.as_deref(), &request.scopes, with_token)
            .map_err(|err| internal(err.code()))
    };
    let dir = request.config_dir();
    fs::create_dir_all(&dir)
        .map_err(|err| OrchestratorError::io(dir.display().to_string(), err.to_string()))?;
    let write = |name: &str, text: String| -> Result<PathBuf, OrchestratorError> {
        let path = dir.join(name);
        fs::write(&path, text)
            .map_err(|err| OrchestratorError::io(path.display().to_string(), err.to_string()))?;
        Ok(path)
    };
    Ok((
        write(INSTALL_CONFIG, render(true)?)?,
        write(LIFECYCLE_CONFIG, render(false)?)?,
    ))
}

/// The private-fetch child: `mise exec bun@<pin> -- bun install ...`.
///
/// # Errors
/// Rejects values the private-fetch constructor refuses.
pub(crate) fn install_command(
    request: &InstallRequest,
    config: &Path,
    token: &Token,
) -> Result<IsolatedCommand, OrchestratorError> {
    let catalog = ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Bun, &request.version)
        .map_err(|err| internal(&format!("bun_registry_version:{err}")))?;
    let mut args: Vec<OsString> = MISE_GLOBAL_FLAGS.iter().map(OsString::from).collect();
    args.extend([
        "exec".into(),
        catalog.tool_spec(PinnedTool::Bun).into(),
        "--".into(),
    ]);
    args.extend([
        "bun".into(),
        "install".into(),
        "--frozen-lockfile".into(),
        "--ignore-scripts".into(),
    ]);
    args.push(format!("--cwd={}", request.work_dir().display()).into());
    args.push(format!("--config={}", config.display()).into());
    let mut owned = request.mise_env.clone();
    owned.push((
        CACHE_ENV.to_owned(),
        request.cache_dir.display().to_string(),
    ));
    let git_config = request.config_dir().join("unused-gitconfig");
    IsolatedCommand::private_fetch(
        "mise",
        args,
        request.work_dir(),
        &git_config,
        &owned,
        Some((PrivateFetchCredential::NpmRegistryRead, token.expose())),
    )
    .map_err(|err| internal(&format!("bun_registry_command:{err}")))
}

/// Run `bun-registry-install-v1` from the step environment.
///
/// # Errors
/// Any configuration, credential or install failure.
pub fn run_bun_registry_install() -> Result<(), OrchestratorError> {
    let get = |key: &str| std::env::var(key).ok();
    let token =
        Token::new(&get(velnor_actions_contract::PRIVATE_NPM_READ_TOKEN_ENV).unwrap_or_default())?;
    let request = InstallRequest::parse(&get)?;
    let (registry, _) = write_configs(&request)?;
    let output = install_command(&request, &registry, &token)?
        .run_bounded(CAPTURE_BYTES, INSTALL_TIMEOUT)
        .map_err(|err| internal(&format!("bun_registry_spawn:{err}")))?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if output.success {
        println!("velnor: bun registry phase complete");
        Ok(())
    } else {
        Err(internal("bun_registry_install_failed"))
    }
}

#[cfg(test)]
#[path = "bun_registry_install_tests.rs"]
mod tests;
