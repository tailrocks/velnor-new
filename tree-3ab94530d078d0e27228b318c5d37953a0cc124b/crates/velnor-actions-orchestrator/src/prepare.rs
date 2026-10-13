//! Validated generation input shared by `plan` and `generate`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{RunnerSelection, VelnorConfig, WorkflowPolicy};
use velnor_actions_mise::GitRequest;

use crate::OrchestratorError;
use crate::config::load_config;
use crate::discover::{Discovery, discover};
use crate::workflow::{DEFAULT_RUNNER_LABEL, WorkflowPlan, build_workflow};

/// Canonical repository identity allowed the Velnor-repository policy.
const VELNOR_IDENTITY: &str = "tailrocks/velnor-new";

/// Everything `plan` and `generate` need: config, branch, discovery, IR.
#[derive(Debug, Clone)]
pub struct GenerationPreparation {
    /// Canonical repository root.
    pub root: PathBuf,
    /// Validated configuration with defaults filled.
    pub config: VelnorConfig,
    /// Push branch: config override or local `origin/HEAD`, never guessed.
    pub default_branch: String,
    /// Literal runner label every job uses.
    pub runner_label: String,
    /// Runner-label selection provenance.
    pub runner_selection: RunnerSelection,
    /// Detection, inventory, profiles, and task groups.
    pub discovery: Discovery,
    /// Workflow IR plus renderer inputs.
    pub workflow: WorkflowPlan,
}

/// Build the shared preparation object for `root`.
///
/// # Errors
///
/// Returns root, config, branch, identity, discovery, or workflow errors.
pub fn prepare(root: &Path) -> Result<GenerationPreparation, OrchestratorError> {
    let canonical = root
        .canonicalize()
        .map_err(|err| OrchestratorError::io(root.display().to_string(), err.to_string()))?;
    if !canonical.is_dir() {
        return Err(OrchestratorError::RootDiscovery {
            problem: format!("not_a_directory:{}", canonical.display()),
        });
    }
    let config = load_config(&canonical)?;
    let default_branch = resolve_default_branch(&canonical, &config)?;
    check_velnor_identity(&canonical, &config)?;
    let discovery = discover(&canonical, &config)?;
    let (runner_label, runner_selection) = runner_label_for(&config);
    let workflow = build_workflow(&config, &default_branch, &runner_label, &discovery)?;
    Ok(GenerationPreparation {
        root: canonical,
        config,
        default_branch,
        runner_label,
        runner_selection,
        discovery,
        workflow,
    })
}

/// Runner label plus provenance from the config override or the default.
pub(crate) fn runner_label_for(config: &VelnorConfig) -> (String, RunnerSelection) {
    match &config.workflow.runner_label {
        Some(label) => (label.clone(), RunnerSelection::ConfigOverride),
        None => (
            DEFAULT_RUNNER_LABEL.to_owned(),
            RunnerSelection::LatestDefault,
        ),
    }
}

/// Default branch from the config override or local `origin/HEAD`.
fn resolve_default_branch(root: &Path, config: &VelnorConfig) -> Result<String, OrchestratorError> {
    if let Some(branch) = &config.workflow.default_branch {
        return Ok(branch.clone());
    }
    let output = GitRequest::rev_parse(vec![
        OsString::from("--abbrev-ref"),
        OsString::from("origin/HEAD"),
    ])
    .run_in(root)
    .map_err(|err| OrchestratorError::DefaultBranch {
        problem: err.to_string(),
    })?;
    if !output.success {
        return Err(OrchestratorError::DefaultBranch {
            problem: "origin_head_unresolvable".to_owned(),
        });
    }
    let text = output
        .stdout_text("git")
        .map_err(|err| OrchestratorError::DefaultBranch {
            problem: err.to_string(),
        })?;
    branch_from_origin_head(text.trim()).ok_or_else(|| OrchestratorError::DefaultBranch {
        problem: "origin_head_unresolvable".to_owned(),
    })
}

/// Strip the `origin/` prefix, rejecting empty or malformed branches.
fn branch_from_origin_head(text: &str) -> Option<String> {
    let branch = text.strip_prefix("origin/").unwrap_or(text);
    if branch.is_empty()
        || branch.contains(char::is_whitespace)
        || branch.contains("..")
        || branch == "HEAD"
    {
        return None;
    }
    Some(branch.to_owned())
}

/// Require the canonical identity for the Velnor-repository policy.
fn check_velnor_identity(root: &Path, config: &VelnorConfig) -> Result<(), OrchestratorError> {
    if config.workflow.policy == WorkflowPolicy::ConsumerV1 {
        return Ok(());
    }
    if std::env::var("GITHUB_REPOSITORY").as_deref() == Ok(VELNOR_IDENTITY) {
        return Ok(());
    }
    if origin_matches(root) {
        return Ok(());
    }
    Err(OrchestratorError::IdentityRejected {
        problem: "velnor_policy_requires_tailrocks_velnor_new".to_owned(),
    })
}

/// True when the local `origin` URL normalizes to the canonical identity.
fn origin_matches(root: &Path) -> bool {
    let Some(config) = git_config_path(root).and_then(|path| std::fs::read_to_string(path).ok())
    else {
        return false;
    };
    config.lines().any(|line| {
        line.split_once('=').is_some_and(|(_, value)| {
            let mut url = value.trim().to_lowercase();
            if let Some(stripped) = url.strip_suffix(".git") {
                url = stripped.to_owned();
            }
            url == VELNOR_IDENTITY || url.ends_with(&format!("/{VELNOR_IDENTITY}"))
        })
    })
}

/// Locate `.git/config`, following a worktree `gitdir` pointer.
fn git_config_path(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    let meta = std::fs::symlink_metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return Some(dot_git.join("config"));
    }
    if !meta.is_file() {
        return None;
    }
    let pointer = std::fs::read_to_string(&dot_git).ok()?;
    let dir = pointer.strip_prefix("gitdir:")?.trim();
    let base = PathBuf::from(dir);
    let resolved = if base.is_absolute() {
        base
    } else {
        dot_git.parent()?.join(base)
    };
    Some(resolved.join("config"))
}
