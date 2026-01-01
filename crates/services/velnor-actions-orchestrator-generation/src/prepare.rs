//! Validated generation input shared by `plan` and `generate`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use velnor_actions_contract_config::{RunnerSelection, VelnorConfig, WorkflowPolicy};
use velnor_actions_contract_release::RunnerImageEvidence;
use velnor_actions_mise::GitRequest;

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::config::load_config;
use velnor_actions_orchestrator_core::decisions::runner_image_evidence;
use velnor_actions_orchestrator_discovery::discover::{Discovery, discover};
use velnor_actions_orchestrator_provisioning::source_prep::lockful_roots;
use velnor_actions_orchestrator_staged_validation::validate::verify_velnor_repository_files;
use velnor_actions_orchestrator_workflow_ir::workflow::{
    DEFAULT_RUNNER_LABEL, WorkflowPlan, build_workflow_with_local_helper_build,
};

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
    /// Whether the Velnor plan will build its helper locally from source.
    /// This also determines whether Rust must be prepared before MBX checks.
    pub local_helper_build: bool,
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
    let local_helper_build = config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1
        && verify_velnor_repository_files(&canonical)?.is_none();
    let mut discovery = discover(&canonical, &config)?;
    let fetch_roots = lockful_roots(&canonical, &discovery.workspaces);
    let (runner_label, runner_selection) = runner_label_for(&config);
    let workflow = build_workflow_with_local_helper_build(
        &config,
        &default_branch,
        &runner_label,
        &discovery,
        local_helper_build,
        &fetch_roots,
    )?;
    let runner_image = runner_image_evidence();
    let audit = velnor_actions_orchestrator_provisioning::lock_audit::audit_prepare_installs(
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
        local_helper_build,
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
        && velnor_actions_orchestrator_core::origin::validate_repository_slug(&hint).as_deref()
            != Some(VELNOR_IDENTITY)
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
/// Resolution runs through the shared [`velnor_actions_orchestrator_core::origin::origin_url_via_git`]
/// helper, so linked worktrees, includes, and worktree configuration all
/// follow Git semantics. Only that one key counts; other sections, other
/// keys, and decoy remotes never grant the identity. A missing or
/// mismatched origin fails; nothing is fetched.
fn origin_matches(root: &Path) -> bool {
    velnor_actions_orchestrator_core::origin::origin_url_via_git(root)
        .is_some_and(|url| url_matches_identity(&url))
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
