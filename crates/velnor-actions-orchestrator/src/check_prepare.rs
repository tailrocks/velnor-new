// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::compiler_observation::{
    ChildOutcome, ObservationReport, ObservationSession, new_session_id,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct TransferLedger {
    pub(crate) origin_acquisitions: u64,
    pub(crate) origin_bytes: u64,
    pub(crate) cache_bytes: u64,
}

impl TransferLedger {
    fn saturating_len(len: usize) -> u64 {
        u64::try_from(len).unwrap_or(u64::MAX)
    }

    pub(crate) fn record_origin(&mut self, bytes: usize) {
        self.origin_acquisitions = self.origin_acquisitions.saturating_add(1);
        self.origin_bytes = self
            .origin_bytes
            .saturating_add(Self::saturating_len(bytes));
    }

    pub(crate) fn record_cache(&mut self, bytes: usize) {
        self.cache_bytes = self.cache_bytes.saturating_add(Self::saturating_len(bytes));
    }
}
use crate::OrchestratorError;
use crate::check_evidence::reject_link_components;
use crate::internal::internal;
use std::ffi::OsStr;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use velnor_actions_contract::digest_b3;
use velnor_actions_mise::checks::SystemToolProof;
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck, QualifiedCheck};

#[path = "check_tool_acquire.rs"]
pub(crate) mod acquisition;
#[path = "check_binary.rs"]
mod binary;
pub(crate) mod cache_execution_context;
#[path = "check_container.rs"]
pub(crate) mod container;

static NEXT_HOME: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub(crate) struct RestoredGroupContext {
    temp: PathBuf,
    roots: cache_execution_context::OwnedExecutionRoots,
    state_root: PathBuf,
}

impl RestoredGroupContext {
    pub(crate) fn resolve(temp: &Path) -> Result<Self, OrchestratorError> {
        let temp = temp.canonicalize().map_err(|_| internal("check_temp"))?;
        let roots = cache_execution_context::resolve_roots(&temp)?;
        let state_root = roots.owned_root.join("state");
        Ok(Self {
            temp,
            roots,
            state_root,
        })
    }

    pub(crate) fn state_root(&self) -> &Path {
        &self.state_root
    }

    pub(crate) fn cargo_home(&self) -> &Path {
        &self.roots.cargo_home
    }

    pub(crate) fn rustup_home(&self) -> &Path {
        &self.roots.rustup_home
    }

    pub(crate) fn tool_root(&self) -> &Path {
        &self.roots.tool_root
    }

    pub(crate) fn context_digest(&self) -> String {
        digest_b3(self.roots.observation_root.to_string_lossy().as_bytes())
    }
}

#[derive(Debug)]
pub(crate) struct OwnedCheck {
    qualified: QualifiedCheck,
    home: PathBuf,
    pub(super) container: Option<container::OwnedContainer>,
    pub(crate) system_tools: Vec<SystemToolProof>,
    pub(crate) qualified_tools: Vec<crate::check_evidence::gate::tools::QualifiedToolReceipt>,
    observation: Option<ObservationSession>,
    transfers: TransferLedger,
}
struct PreparedParts {
    qualified: QualifiedCheck,
    container: Option<container::OwnedContainer>,
    system_tools: Vec<SystemToolProof>,
    qualified_tools: Vec<crate::check_evidence::gate::tools::QualifiedToolReceipt>,
    transfers: TransferLedger,
}
impl Deref for OwnedCheck {
    type Target = QualifiedCheck;
    fn deref(&self) -> &Self::Target {
        &self.qualified
    }
}
impl Drop for OwnedCheck {
    fn drop(&mut self) {
        cache_execution_context::discard_private_home(&self.home);
    }
}

pub(crate) fn prepare_check(
    root: &Path,
    temp: &Path,
    check: &DiscoveredCheck,
    deadline: CheckDeadline,
) -> Result<OwnedCheck, OrchestratorError> {
    let group = RestoredGroupContext::resolve(temp)?;
    prepare_check_with_context(root, &group, check, deadline)
}

pub(crate) fn prepare_check_with_context(
    root: &Path,
    group: &RestoredGroupContext,
    check: &DiscoveredCheck,
    deadline: CheckDeadline,
) -> Result<OwnedCheck, OrchestratorError> {
    checkpoint(deadline)?;
    verify_source(root, check, deadline)?;
    checkpoint(deadline)?;
    let cwd = velnor_actions_mise::checks::repository_path(root, &check.check.directory)
        .map_err(|e| internal(&e.to_string()))?;
    let root = root.canonicalize().map_err(|_| internal("check_root"))?;
    if group.temp.starts_with(&root) {
        return Err(internal("check_temp_inside_repository"));
    }
    let runtime = binary::resolve_mise_runtime(&root, check, deadline)?;
    let n = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
    let home = group
        .temp
        .join(format!("velnor-check-{}-{n}", std::process::id()));
    std::fs::create_dir(&home).map_err(|_| internal("check_home_creation"))?;
    if let Err(error) = cache_execution_context::ensure_private_placement(&root, &group.temp, &home)
    {
        cache_execution_context::discard_private_home(&home);
        return Err(error);
    }
    let observation = ObservationSession::begin(
        &check.proposal.task_id,
        &check.check.id,
        &new_session_id(),
        &group.context_digest(),
    )
    .map_err(|error| internal(&error.to_string()))?;
    let prepared = materialize(group, &home, cwd, check, &runtime, deadline);
    match prepared {
        Ok(parts) => Ok(OwnedCheck {
            qualified: parts.qualified,
            home,
            container: parts.container,
            system_tools: parts.system_tools,
            qualified_tools: parts.qualified_tools,
            observation: Some(observation),
            transfers: parts.transfers,
        }),
        Err(error) => {
            cache_execution_context::discard_private_home(&home);
            Err(error)
        }
    }
}

impl OwnedCheck {
    pub(super) fn finish_observation(
        &mut self,
        outcome: ChildOutcome,
    ) -> Result<ObservationReport, OrchestratorError> {
        let mut session = self
            .observation
            .take()
            .ok_or_else(|| internal("observation_session_missing"))?;
        let compiler = session
            .compiler_evidence(
                None,
                None,
                None,
                None,
                None,
                "rust compiler invocations unattributed on this execution path",
            )
            .map_err(|error| internal(&error.to_string()))?;
        session
            .provide_compiler(compiler)
            .map_err(|error| internal(&error.to_string()))?;
        let native = session
            .native_evidence(
                None,
                None,
                None,
                None,
                "native compiler calls unobserved on this execution path",
            )
            .map_err(|error| internal(&error.to_string()))?;
        session
            .provide_native(native)
            .map_err(|error| internal(&error.to_string()))?;
        let origin = session
            .origin_evidence(
                Some(self.transfers.origin_acquisitions),
                Some(self.transfers.origin_bytes),
                "tool artifacts fetched from origin during materialization",
            )
            .map_err(|error| internal(&error.to_string()))?;
        session
            .provide_origin(origin)
            .map_err(|error| internal(&error.to_string()))?;
        let cache = session
            .cache_evidence(
                Some(self.transfers.cache_bytes),
                "tool artifacts reused from the shared archive cache",
            )
            .map_err(|error| internal(&error.to_string()))?;
        session
            .provide_cache(cache)
            .map_err(|error| internal(&error.to_string()))?;
        session
            .finish(outcome)
            .map_err(|error| internal(&error.to_string()))
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
    group: &RestoredGroupContext,
    home: &Path,
    cwd: PathBuf,
    check: &DiscoveredCheck,
    runtime: &velnor_actions_mise::catalog::lock::ApprovedMiseRuntime,
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
    binary::project_mise_binary(home, check.check.runner.platform, runtime, deadline)?;
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
            velnor_actions_contract::config::CheckSystemToolKind::Swift => "swift",
            velnor_actions_contract::config::CheckSystemToolKind::Xcode => "xcodebuild",
        };
        link_program(OsStr::new(&proof.executable), &home.join("bin").join(name))?;
    }
    let mut qualified = QualifiedCheck::new(
        home.to_path_buf(),
        cwd,
        check.clone(),
        proof.proof,
        &system_tools,
        runtime.version.clone(),
    )
    .map_err(|e| internal(&e.to_string()))?;
    qualified
        .project_restored_homes(
            group.state_root(),
            group.cargo_home(),
            group.rustup_home(),
            group.tool_root(),
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
    let (qualified_tools, transfers) =
        acquisition::acquire(&qualified, check, home, deadline)?;
    Ok(PreparedParts {
        qualified,
        container,
        system_tools,
        qualified_tools,
        transfers,
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
