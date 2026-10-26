// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{RunnerImageEvidence, RunnerSelection, VelnorConfig, WorkflowPolicy};
use velnor_actions_mise::{GitRequest, ToolCatalog};

use crate::OrchestratorError;
use crate::config::load_config;
use crate::decisions::runner_image_evidence;
use crate::discover::{Discovery, discover, discover_with_phase_timings};
use crate::internal::phase_timing::PlanPhaseTimings;
use crate::source_prep::lockful_roots;
use crate::workflow::{DEFAULT_RUNNER_LABEL, WorkflowPlan};

const VELNOR_IDENTITY: &str = "tailrocks/velnor-new";

#[derive(Debug, Clone)]
pub struct GenerationPreparation {
    pub root: PathBuf,
    pub config: VelnorConfig,
    pub default_branch: String,
    pub runner_label: String,
    pub runner_selection: RunnerSelection,
    pub discovery: Discovery,
    pub workflow: WorkflowPlan,
    pub runner_image: RunnerImageEvidence,
    pub lock_audit_blocking: Vec<String>,
    pub catalog: ToolCatalog,
}

impl GenerationPreparation {
    #[must_use]
    pub fn emission_catalog(&self) -> ToolCatalog {
        self.catalog.for_emission()
    }
}

pub fn prepare(root: &Path) -> Result<GenerationPreparation, OrchestratorError> {
    prepare_inner(root, None, None)
}

pub(crate) fn prepare_with_config(
    root: &Path,
    config: VelnorConfig,
) -> Result<GenerationPreparation, OrchestratorError> {
    config
        .validate(crate::config::CONFIG_REL)
        .map_err(crate::config::config_error)?;
    crate::config::check_policy_mode(&config)?;
    prepare_inner(root, None, Some(config))
}

pub(crate) fn prepare_with_phase_timings(
    root: &Path,
    phases: &mut PlanPhaseTimings,
) -> Result<GenerationPreparation, OrchestratorError> {
    prepare_inner(root, Some(phases), None)
}

fn prepare_inner(
    root: &Path,
    phases: Option<&mut PlanPhaseTimings>,
    admitted_config: Option<VelnorConfig>,
) -> Result<GenerationPreparation, OrchestratorError> {
    let canonical = root
        .canonicalize()
        .map_err(|err| OrchestratorError::io(root.display().to_string(), err.to_string()))?;
    if !canonical.is_dir() {
        return Err(OrchestratorError::RootDiscovery {
            problem: format!("not_a_directory:{}", canonical.display()),
        });
    }
    let config = match admitted_config {
        Some(config) => config,
        None => load_config(&canonical)?,
    };
    let catalog = crate::tool_catalog_resolve::resolve_tool_catalog(&canonical, &config)?;
    let default_branch = resolve_default_branch(&canonical, &config)?;
    check_velnor_identity(&canonical, &config)?;
    let mut discovery = match phases {
        Some(phases) => discover_with_phase_timings(&canonical, &config, &catalog, phases)?,
        None => discover(&canonical, &config, &catalog)?,
    };
    let fetch_roots = lockful_roots(&canonical, &discovery.workspaces);
    let (runner_label, runner_selection) = runner_label_for(&config);
    let emission = catalog.for_emission();
    let workflow = crate::workflow::build_workflow_for_consumer_release(
        &crate::workflow::WorkflowBuildInput {
            root: &canonical,
            config: &config,
            branch: &default_branch,
            label: &runner_label,
            discovery: &discovery,
            fetch_roots: &fetch_roots,
            consumer_release_version: env!("CARGO_PKG_VERSION"),
            catalog: &emission,
        },
    )?;
    let verifier_actions = crate::workflow::verifier_actions::VerifierActionSet::build(
        &emission,
        env!("CARGO_PKG_VERSION"),
    )?;
    let runner_image = runner_image_evidence();
    let audit = crate::lock_audit::audit_prepare_installs_with_catalog_and_verifiers(
        &canonical,
        &workflow.ir,
        &runner_label,
        &workflow.context.validator_commands,
        &catalog,
        &verifier_actions,
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
        catalog,
    })
}

pub(crate) fn runner_label_for(config: &VelnorConfig) -> (String, RunnerSelection) {
    match &config.workflow.runner_label {
        Some(label) => (label.clone(), RunnerSelection::ConfigOverride),
        None => (
            DEFAULT_RUNNER_LABEL.to_owned(),
            RunnerSelection::LatestDefault,
        ),
    }
}

fn resolve_default_branch(root: &Path, config: &VelnorConfig) -> Result<String, OrchestratorError> {
    if let Some(branch) = &config.workflow.default_branch {
        return Ok(branch.clone());
    }
    if let Some(branch) = origin_head_branch(root)? {
        return Ok(branch);
    }
    if let Some(branch) = event_default_branch()? {
        return Ok(branch);
    }
    Err(OrchestratorError::DefaultBranch {
        problem: "origin_head_unresolvable:set workflow.default_branch".to_owned(),
    })
}

fn origin_head_branch(root: &Path) -> Result<Option<String>, OrchestratorError> {
    let output = GitRequest::rev_parse(vec![
        OsString::from("--abbrev-ref"),
        OsString::from("origin/HEAD"),
    ])
    .run_in(root)
    .map_err(|err| OrchestratorError::DefaultBranch {
        problem: err.to_string(),
    })?;
    if !output.success {
        return Ok(None);
    }
    let text = output
        .stdout_text("git")
        .map_err(|err| OrchestratorError::DefaultBranch {
            problem: err.to_string(),
        })?;
    Ok(branch_from_origin_head(text.trim()))
}

fn event_default_branch() -> Result<Option<String>, OrchestratorError> {
    let Some(path) = std::env::var_os("GITHUB_EVENT_PATH").filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let payload =
        crate::safe_read::read_event_file(Path::new(&path), crate::safe_read::MAX_REPO_FILE_BYTES)?;
    Ok(branch_from_event_payload(&payload))
}

fn branch_from_event_payload(payload: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let branch = value.get("repository")?.get("default_branch")?.as_str()?;
    velnor_actions_contract::is_valid_branch_name(branch).then(|| branch.to_owned())
}

fn branch_from_origin_head(text: &str) -> Option<String> {
    let branch = text.strip_prefix("origin/").unwrap_or(text);
    velnor_actions_contract::is_valid_branch_name(branch).then(|| branch.to_owned())
}

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

fn origin_matches(root: &Path) -> bool {
    crate::origin::origin_url_via_git(root).is_some_and(|url| url_matches_identity(&url))
}

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
    let (before_colon, path) = url.split_once(':')?;
    let host = before_colon.rsplit('@').next().unwrap_or(before_colon);
    if host.is_empty() || host.contains('/') || path.is_empty() || path.starts_with('/') {
        return None;
    }
    Some((host, path))
}
