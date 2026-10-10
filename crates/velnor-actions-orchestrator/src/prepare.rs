//! Validated generation input shared by `plan` and `generate`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{RunnerImageEvidence, RunnerSelection, VelnorConfig, WorkflowPolicy};
use velnor_actions_mise::GitRequest;

use crate::OrchestratorError;
use crate::config::load_config;
use crate::decisions::runner_image_evidence;
use crate::discover::{Discovery, discover, discover_with_phase_timings};
use crate::internal::phase_timing::PlanPhaseTimings;
use crate::source_prep::lockful_roots;
use crate::workflow::{DEFAULT_RUNNER_LABEL, WorkflowPlan};

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
    /// Runner-image evidence for the label: explicitly unobserved at
    /// generation time; observed provisioner facts bind later (VER-4.2).
    pub runner_image: RunnerImageEvidence,
    /// Install lockfile audit blockers (G2): CI-platform holes and
    /// corrupt checksums. `generate` fails closed on any entry while
    /// `plan` reports them; also mirrored into recommendations.
    pub lock_audit_blocking: Vec<String>,
}

/// Build the shared preparation object for `root`.
///
/// # Errors
///
/// Returns root, config, branch, identity, discovery, or workflow errors.
pub fn prepare(root: &Path) -> Result<GenerationPreparation, OrchestratorError> {
    prepare_inner(root, None)
}

/// Prepare an exact-consumer diagnostic using its committed helper release pin.
///
/// This exists only for the ignored full-tree capture harness: the harness
/// renders a consumer pinned to an already published release while testing
/// newer generator source. Normal `prepare` continues to bind the generator's
/// own package version.
#[cfg(all(feature = "test-render-capture", test))]
pub(super) fn prepare_for_capture(
    root: &Path,
    consumer_release_version: &str,
) -> Result<GenerationPreparation, OrchestratorError> {
    prepare_inner_with_consumer_release_version(root, None, consumer_release_version)
}

pub(crate) fn prepare_with_phase_timings(
    root: &Path,
    phases: &mut PlanPhaseTimings,
) -> Result<GenerationPreparation, OrchestratorError> {
    prepare_inner(root, Some(phases))
}

fn prepare_inner(
    root: &Path,
    phases: Option<&mut PlanPhaseTimings>,
) -> Result<GenerationPreparation, OrchestratorError> {
    prepare_inner_with_consumer_release_version(root, phases, env!("CARGO_PKG_VERSION"))
}

fn prepare_inner_with_consumer_release_version(
    root: &Path,
    phases: Option<&mut PlanPhaseTimings>,
    consumer_release_version: &str,
) -> Result<GenerationPreparation, OrchestratorError> {
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
    let mut discovery = match phases {
        Some(phases) => discover_with_phase_timings(&canonical, &config, phases)?,
        None => discover(&canonical, &config)?,
    };
    let fetch_roots = lockful_roots(&canonical, &discovery.workspaces);
    let (runner_label, runner_selection) = runner_label_for(&config);
    let workflow = crate::workflow::build_workflow_for_consumer_release(
        &canonical,
        &config,
        &default_branch,
        &runner_label,
        &discovery,
        &fetch_roots,
        consumer_release_version,
    )?;
    let runner_image = runner_image_evidence();
    let audit = crate::lock_audit::audit_prepare_installs(
        &canonical,
        &workflow.ir,
        &runner_label,
        &workflow.context.validator_commands,
    );
    discovery.recommendations.extend(audit.recommendation);
    discovery
        .recommendations
        .extend(audit.blocking.iter().cloned());
    discovery.recommendations.sort();
    discovery.recommendations.dedup();
    Ok(GenerationPreparation {
        root: canonical,
        config,
        default_branch,
        runner_label,
        runner_selection,
        discovery,
        workflow,
        runner_image,
        lock_audit_blocking: audit.blocking,
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
    velnor_actions_contract::is_valid_branch_name(branch).then(|| branch.to_owned())
}

/// Require the canonical identity for the Velnor-repository policy.
///
/// The local `origin` URL is the authority. `GITHUB_REPOSITORY` is only a
/// consistency hint: a mismatch fails closed and never unlocks, and a
/// matching hint without a canonical origin unlocks nothing either.
pub(crate) fn check_velnor_identity(
    root: &Path,
    config: &VelnorConfig,
) -> Result<(), OrchestratorError> {
    if config.workflow.policy == WorkflowPolicy::ConsumerV1 {
        return Ok(());
    }
    if let Ok(hint) = std::env::var("GITHUB_REPOSITORY")
        && crate::origin::validate_repository_slug(&hint).as_deref() != Some(VELNOR_IDENTITY)
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
/// Resolution runs through the shared [`crate::origin::origin_url_via_git`]
/// helper, so linked worktrees, includes, and worktree configuration all
/// follow Git semantics. Only that one key counts; other sections, other
/// keys, and decoy remotes never grant the identity. A missing or
/// mismatched origin fails; nothing is fetched.
fn origin_matches(root: &Path) -> bool {
    crate::origin::origin_url_via_git(root).is_some_and(|url| url_matches_identity(&url))
}

/// True when a remote URL normalizes to the canonical identity.
///
/// Both host and path must match `github.com/tailrocks/velnor-new` exactly
/// (after lowercasing and trimming an optional `.git` suffix and trailing
/// slashes). A bare `tailrocks/velnor-new` grants nothing: a local-path
/// remote is trivially forgeable via `git remote set-url`. A path-suffix
/// match alone is rejected: any host that merely ends in the identity
/// path (e.g. `evil.example/tailrocks/velnor-new`) grants nothing.
fn url_matches_identity(value: &str) -> bool {
    let mut url = value.trim().to_lowercase();
    while url.ends_with('/') {
        url.pop();
    }
    if let Some(stripped) = url.strip_suffix(".git") {
        url = stripped.to_owned();
    }
    let Some((host, path)) = split_host_path(&url) else {
        return false;
    };
    host == "github.com" && path == VELNOR_IDENTITY
}

/// Split a remote URL into `(host, path)` for `scheme://` and scp forms.
pub(crate) fn split_host_path(url: &str) -> Option<(&str, &str)> {
    if let Some((_scheme, rest)) = url.split_once("://") {
        let after_user = rest.rsplit('@').next().unwrap_or(rest);
        let (host, path) = after_user.split_once('/')?;
        let mut path = path;
        while path.ends_with('/') {
            path = path.strip_suffix('/').unwrap_or(path);
        }
        return Some((host, path));
    }
    // scp-like `host:path` (user part already stripped by caller split).
    let (before_colon, path) = url.split_once(':')?;
    let host = before_colon.rsplit('@').next().unwrap_or(before_colon);
    if host.is_empty() || host.contains('/') || path.is_empty() || path.starts_with('/') {
        return None;
    }
    Some((host, path))
}

#[cfg(test)]
#[path = "prepare_branch_tests.rs"]
mod prepare_branch_tests;
