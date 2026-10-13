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
            problem: "origin_head_unresolvable:set workflow.default_branch".to_owned(),
        });
    }
    let text = output
        .stdout_text("git")
        .map_err(|err| OrchestratorError::DefaultBranch {
            problem: err.to_string(),
        })?;
    branch_from_origin_head(text.trim()).ok_or_else(|| OrchestratorError::DefaultBranch {
        problem: "origin_head_unresolvable:set workflow.default_branch".to_owned(),
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
///
/// The local `origin` URL is the authority. `GITHUB_REPOSITORY` is only a
/// consistency hint: a mismatch fails closed and never unlocks, and a
/// matching hint without a canonical origin unlocks nothing either.
fn check_velnor_identity(root: &Path, config: &VelnorConfig) -> Result<(), OrchestratorError> {
    if config.workflow.policy == WorkflowPolicy::ConsumerV1 {
        return Ok(());
    }
    if let Ok(hint) = std::env::var("GITHUB_REPOSITORY")
        && hint != VELNOR_IDENTITY
    {
        return Err(OrchestratorError::IdentityRejected {
            problem: "github_repository_mismatch:velnor_policy_requires_tailrocks_velnor_new"
                .to_owned(),
        });
    }
    if origin_matches(root) {
        return Ok(());
    }
    Err(OrchestratorError::IdentityRejected {
        problem: "velnor_policy_requires_tailrocks_velnor_new".to_owned(),
    })
}

/// True when the local `origin` URL normalizes to the canonical identity.
///
/// The git directory resolves through the Mise git helper and only the
/// `url` key inside the `[remote "origin"]` section counts; other
/// sections, other keys, and decoy remotes never grant the identity.
/// Nothing is fetched: local config only.
fn origin_matches(root: &Path) -> bool {
    let Ok(output) = GitRequest::rev_parse(vec![OsString::from("--absolute-git-dir")]).run_in(root)
    else {
        return false;
    };
    if !output.success {
        return false;
    }
    let Ok(git_dir) = output.stdout_text("git") else {
        return false;
    };
    let Ok(config) = std::fs::read_to_string(PathBuf::from(git_dir.trim()).join("config")) else {
        return false;
    };
    let mut in_origin = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if let Some(header) = trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            in_origin = is_origin_section(header.trim());
            continue;
        }
        if !in_origin {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("url") {
            continue;
        }
        if url_matches_identity(value.trim()) {
            return true;
        }
    }
    false
}

/// True for the `[remote "origin"]` section header (case-insensitive section).
fn is_origin_section(header: &str) -> bool {
    let mut parts = header.splitn(2, char::is_whitespace);
    let section = parts.next().unwrap_or_default();
    let subsection = parts.next().unwrap_or_default().trim();
    section.eq_ignore_ascii_case("remote") && subsection == "\"origin\""
}

/// True when a remote URL normalizes to the canonical identity.
fn url_matches_identity(value: &str) -> bool {
    let mut url = value.to_lowercase();
    if let Some(stripped) = url.strip_suffix(".git") {
        url = stripped.to_owned();
    }
    url == VELNOR_IDENTITY || url.ends_with(&format!("/{VELNOR_IDENTITY}"))
}
