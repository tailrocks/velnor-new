//! Runtime owns materialized isolated check homes; adapters only inspect/execute.
use crate::OrchestratorError;
use crate::check_evidence::reject_link_components;
use crate::internal::internal;
use std::ffi::OsStr;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use velnor_actions_mise::checks::SystemToolProof;
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, QualifiedCheck};

mod acquisition;
mod binary;
pub(crate) mod container;

static NEXT_HOME: AtomicU64 = AtomicU64::new(0);

/// Own cleanup while the immutable typed command handle lives.
#[derive(Debug)]
pub(super) struct OwnedCheck {
    qualified: QualifiedCheck,
    home: PathBuf,
    pub(super) container: Option<container::OwnedContainer>,
    pub(super) system_tools: Vec<SystemToolProof>,
    pub(super) qualified_tools: Vec<crate::check_evidence::gate::tools::QualifiedToolReceipt>,
}
struct PreparedParts {
    qualified: QualifiedCheck,
    container: Option<container::OwnedContainer>,
    system_tools: Vec<SystemToolProof>,
    qualified_tools: Vec<crate::check_evidence::gate::tools::QualifiedToolReceipt>,
}
impl Deref for OwnedCheck {
    type Target = QualifiedCheck;
    fn deref(&self) -> &Self::Target {
        &self.qualified
    }
}
impl Drop for OwnedCheck {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.home) {
            eprintln!("velnor_check_cleanup_failed:{:?}", error.kind());
        }
    }
}

/// Materialize one source-bound projection and credential-free tool home.
pub(super) fn prepare_check(
    root: &Path,
    temp: &Path,
    check: &DiscoveredCheck,
    deadline: CheckDeadline,
) -> Result<OwnedCheck, OrchestratorError> {
    checkpoint(deadline)?;
    verify_source(root, check, deadline)?;
    checkpoint(deadline)?;
    let cwd = velnor_actions_mise::checks::repository_path(root, &check.check.directory)
        .map_err(|e| internal(&e.to_string()))?;
    let root = root.canonicalize().map_err(|_| internal("check_root"))?;
    let temp = temp.canonicalize().map_err(|_| internal("check_temp"))?;
    if temp.starts_with(&root) {
        return Err(internal("check_temp_inside_repository"));
    }
    let n = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
    let home = temp.join(format!("velnor-check-{}-{n}", std::process::id()));
    std::fs::create_dir(&home).map_err(|_| internal("check_home_creation"))?;
    let prepared = materialize(&home, cwd, check, deadline);
    match prepared {
        Ok(parts) => Ok(OwnedCheck {
            qualified: parts.qualified,
            home,
            container: parts.container,
            system_tools: parts.system_tools,
            qualified_tools: parts.qualified_tools,
        }),
        Err(error) => {
            if let Err(cleanup) = std::fs::remove_dir_all(&home) {
                eprintln!("velnor_check_cleanup_failed:{:?}", cleanup.kind());
            }
            Err(error)
        }
    }
}

fn verify_source(
    root: &Path,
    check: &DiscoveredCheck,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    checkpoint(deadline)?;
    let path = &check.proposal.identity.unit_path;
    reject_link_components(root, path)?;
    match crate::safe_read::read_repo_file_until(
        root,
        path,
        crate::safe_read::MAX_REPO_FILE_BYTES,
        deadline,
    )? {
        crate::safe_read::RepoRead::Text(current) if current == check.config_source => Ok(()),
        _ => Err(internal("check_source_changed_since_discovery")),
    }
}

fn materialize(
    home: &Path,
    cwd: PathBuf,
    check: &DiscoveredCheck,
    deadline: CheckDeadline,
) -> Result<PreparedParts, OrchestratorError> {
    for name in [
        "data",
        "cache",
        "state",
        "config",
        "cargo",
        velnor_actions_mise::checks::RUSTUP_HOME_SUFFIX,
        "bin",
        "docker",
    ] {
        checkpoint(deadline)?;
        std::fs::create_dir(home.join(name)).map_err(|_| internal("check_home_creation"))?;
        checkpoint(deadline)?;
    }
    binary::project_mise_binary(home, check.check.runner.platform, deadline)?;
    crate::exclusive_write::write_exclusive_until(
        &home.join("empty.toml"),
        b"",
        "check_config",
        || checkpoint(deadline),
    )?;
    let container = container::prepare(home, &check.check.runner, deadline)?;
    let proof = container::probe(&check.check.runner, container.as_ref(), deadline)?;
    let system_tools = velnor_actions_mise::checks::verify_check_system_tools(
        check.check.runner.platform,
        &check.check.system_tools,
        deadline,
    )
    .map_err(|e| internal(&e.to_string()))?;
    for proof in &system_tools {
        let name = match proof.declared.kind {
            velnor_actions_contract_config::config::CheckSystemToolKind::Swift => "swift",
            velnor_actions_contract_config::config::CheckSystemToolKind::Xcode => "xcodebuild",
        };
        link_program(OsStr::new(&proof.executable), &home.join("bin").join(name))?;
    }
    let qualified = QualifiedCheck::new(
        home.to_path_buf(),
        cwd,
        check.clone(),
        proof.proof,
        &system_tools,
    )
    .map_err(|e| internal(&e.to_string()))?;
    let projection = qualified
        .bound_projection()
        .map_err(|e| internal(&e.to_string()))?;
    crate::exclusive_write::write_exclusive_until(
        &home.join("tasks.toml"),
        projection.as_bytes(),
        "check_config",
        || checkpoint(deadline),
    )?;
    let qualified_tools = acquisition::acquire(&qualified, check, home, deadline)?;
    Ok(PreparedParts {
        qualified,
        container,
        system_tools,
        qualified_tools,
    })
}

fn checkpoint(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|error| internal(&error.to_string()))
}

fn link_program(program: &OsStr, destination: &Path) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(program, destination)
            .map_err(|_| internal("check_binary_projection"))
    }
    #[cfg(not(unix))]
    {
        let _ = (program, destination);
        Err(internal("check_binary_projection_platform"))
    }
}

#[cfg(test)]
mod tests;
