//! Standalone workflows for credential-free, compile-capable Mise tasks.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    GitHubEvidenceFetchSpecV1, MiseTaskSuite, MiseTaskSuiteEvent, RunsOn, RustSuiteArtifact,
};

use crate::verification_jobs::{
    MiseTaskBootstrapPolicy, build_task_jobs::BuildTaskTool, mise_task_bootstrap,
};
use crate::{MiseSetup, RenderError, RenderedFile, Yaml, render, steps};

#[path = "mise_task_suites_evidence.rs"]
mod evidence;
pub use evidence::EvidenceHelperProvenance;
use evidence::{
    evidence_bundle_name, evidence_collector_condition, evidence_fetch_job_id,
    evidence_fetch_job_yaml, github_evidence_internal_step, job_step_to_yaml,
};

/// One orchestrator-resolved task suite and its locked tool closure.
#[derive(Debug, Clone)]
pub struct MiseTaskSuitePolicy {
    /// Validated config declaration.
    pub suite: MiseTaskSuite,
    /// Exact fixed runner label.
    pub runner_label: String,
    /// Pinned Mise action and binary digest for the runner target.
    pub mise_setup: MiseSetup,
    /// Selected exact prebuilt tools from the source Mise lock.
    pub selected_tools: Vec<BuildTaskTool>,
    /// SHA-256 of the task Mise config, when present.
    pub mise_config_sha256: Option<String>,
    /// SHA-256 of the task Mise lock, when present.
    pub mise_lock_sha256: Option<String>,
    /// SHA-256 of the task Rust toolchain file, when present.
    pub rust_toolchain_sha256: Option<String>,
    /// Canonical closed GitHub snapshot specification, when enabled.
    pub evidence_fetch_spec: Option<GitHubEvidenceFetchSpecV1>,
    /// Validated immutable release provenance for the fixed evidence helper steps.
    pub evidence_helper: Option<EvidenceHelperProvenance>,
}

/// Inputs required to render task-suite companion workflow files.
#[derive(Debug, Clone)]
pub struct MiseTaskSuitesRenderSpec {
    /// Sorted task-suite policy inventory.
    pub suites: Vec<MiseTaskSuitePolicy>,
    /// Push suites run only on this branch.
    pub default_branch: String,
    /// Pinned checkout action reference.
    pub checkout_uses: String,
    /// Generator version in ownership markers.
    pub generator_version: String,
}

/// Render every standalone `.github/workflows/<slug>.yml` task-suite file.
/// # Errors
/// Returns [`RenderError`] when a suite, action, or generated workflow is invalid.
pub fn render_mise_task_suites(
    spec: &MiseTaskSuitesRenderSpec,
) -> Result<Vec<RenderedFile>, RenderError> {
    validate_spec(spec)?;
    let mut grouped: BTreeMap<&str, Vec<&MiseTaskSuitePolicy>> = BTreeMap::new();
    for suite in &spec.suites {
        grouped
            .entry(suite.suite.workflow.as_str())
            .or_default()
            .push(suite);
    }
    grouped
        .into_iter()
        .map(|(slug, suites)| {
            let yaml = render_workflow(slug, &suites, spec)?;
            Ok(RenderedFile {
                path: format!(".github/workflows/{slug}.yml"),
                bytes: yaml,
            })
        })
        .collect()
}

fn validate_spec(spec: &MiseTaskSuitesRenderSpec) -> Result<(), RenderError> {
    crate::marker::validate_version(&spec.generator_version)?;
    if !velnor_actions_contract::is_valid_branch_name(&spec.default_branch) {
        return Err(RenderError::InvalidWorkflow(
            "task_suite_default_branch".to_owned(),
        ));
    }
    steps::validate_uses(&spec.checkout_uses)?;
    if !spec.checkout_uses.starts_with("actions/checkout@") {
        return Err(RenderError::BadActionRef(format!(
            "not_checkout:{}",
            spec.checkout_uses
        )));
    }
    let mut last_id = None;
    for policy in &spec.suites {
        policy
            .suite
            .validate(".velnor/config.toml", "workflow.task_suites")
            .map_err(RenderError::Contract)?;
        if last_id.is_some_and(|previous: &str| previous >= policy.suite.id.as_str()) {
            return Err(RenderError::InvalidWorkflow(
                "task_suites_not_sorted_unique".to_owned(),
            ));
        }
        last_id = Some(policy.suite.id.as_str());
        if policy.runner_label != policy.suite.runner.runs_on() {
            return Err(RenderError::InvalidWorkflow(format!(
                "task_suite_runner_mismatch:{}",
                policy.suite.id
            )));
        }
        policy.mise_setup.validate()?;
        evidence::validate_policy(policy, spec)?;
        let bootstrap = bootstrap_policy(policy);
        mise_task_bootstrap::validate_policy(&bootstrap)?;
    }
    Ok(())
}

fn render_workflow(
    slug: &str,
    suites: &[&MiseTaskSuitePolicy],
    spec: &MiseTaskSuitesRenderSpec,
) -> Result<String, RenderError> {
    let mut jobs = Vec::with_capacity(suites.len());
    for policy in suites {
        if policy.evidence_fetch_spec.is_some() {
            jobs.push((
                evidence_fetch_job_id(&policy.suite.id),
                evidence_fetch_job_yaml(policy, spec)?,
            ));
        }
        let condition = if policy.evidence_fetch_spec.is_some() {
            evidence_collector_condition(&policy.suite, &spec.default_branch)
        } else {
            suite_condition(&policy.suite, &spec.default_branch)
        };
        let bootstrap = bootstrap_policy(policy);
        let mut task_steps = mise_task_bootstrap::steps(
            &bootstrap,
            &policy.mise_setup,
            &spec.checkout_uses,
            true,
            true,
        )?;
        if let Some(evidence_spec) = &policy.evidence_fetch_spec {
            let helper = policy.evidence_helper.as_ref().ok_or_else(|| {
                RenderError::InvalidWorkflow("github_evidence_helper_missing".to_owned())
            })?;
            let mut evidence_steps = vec![helper.acquire_step()?];
            evidence_steps.push(crate::steps::download_artifact_step(
                &evidence_bundle_name(&policy.suite.id),
                "${{ runner.temp }}/velnor/github-evidence-bundle",
            )?);
            evidence_steps.push(github_evidence_internal_step(
                velnor_actions_contract::GITHUB_EVIDENCE_IMPORT_OPERATION,
                evidence_spec,
                None,
            )?);
            task_steps.splice(1..1, evidence_steps);
        }
        let mut rendered_steps = task_steps
            .iter()
            .map(|step| job_step_to_yaml(policy, step))
            .collect::<Result<Vec<_>, _>>()?;
        for artifact in &policy.suite.artifacts {
            rendered_steps.push(artifact_step(policy, artifact)?);
        }
        let job_id = format!("mise-task-{}", policy.suite.id);
        jobs.push((
            job_id,
            job_yaml(
                policy,
                &condition,
                rendered_steps,
                policy
                    .evidence_fetch_spec
                    .as_ref()
                    .map(|_| evidence_fetch_job_id(&policy.suite.id)),
            )?,
        ));
    }
    let top = vec![
        ("name".to_owned(), Yaml::str(format!("Mise tasks ({slug})"))),
        ("on".to_owned(), triggers_yaml(suites, &spec.default_branch)),
        ("permissions".to_owned(), contents_read_permissions()),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ];
    let document = Yaml::Map(top);
    let path = format!(".github/workflows/{slug}.yml");
    let text =
        render::fallback::render_checked_workflow(&path, &document, &spec.generator_version)?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(text)
}

fn bootstrap_policy(policy: &MiseTaskSuitePolicy) -> MiseTaskBootstrapPolicy {
    MiseTaskBootstrapPolicy {
        id: policy.suite.id.clone(),
        mise_task: policy.suite.mise_task.clone(),
        source: policy.suite.source.clone(),
        runner: policy.suite.runner,
        selected_tools: policy.selected_tools.clone(),
        mise_config_sha256: policy.mise_config_sha256.clone(),
        mise_lock_sha256: policy.mise_lock_sha256.clone(),
        rust_toolchain_sha256: policy.rust_toolchain_sha256.clone(),
        root_prefix: "mise-task-suite",
        run_step_name: mise_task_bootstrap::RUN_MISE_TASK_SUITE_NAME,
        evidence_input: policy.evidence_fetch_spec.is_some(),
    }
}

fn job_yaml(
    policy: &MiseTaskSuitePolicy,
    condition: &str,
    rendered_steps: Vec<Yaml>,
    needs: Option<String>,
) -> Result<Yaml, RenderError> {
    let runner = RunsOn::parse(&policy.runner_label).map_err(RenderError::Contract)?;
    let runs_on = match runner {
        RunsOn::Hosted(label) => Yaml::str(label),
        RunsOn::ScaleSet(selector) => Yaml::Flow(selector.labels().to_vec()),
    };
    steps::scan_for_private_subcommands(condition)?;
    let mut entries = vec![
        (
            "name".to_owned(),
            Yaml::str(format!("Mise task {}", policy.suite.id)),
        ),
        ("runs-on".to_owned(), runs_on),
    ];
    if let Some(needs) = needs {
        entries.push(("needs".to_owned(), Yaml::Seq(vec![Yaml::str(needs)])));
    }
    entries.extend([
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(policy.suite.timeout_minutes)),
        ),
        ("permissions".to_owned(), contents_read_permissions()),
        ("if".to_owned(), Yaml::str(condition.to_owned())),
        ("steps".to_owned(), Yaml::Seq(rendered_steps)),
    ]);
    Ok(Yaml::Map(entries))
}

fn contents_read_permissions() -> Yaml {
    Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))])
}

fn suite_condition(suite: &MiseTaskSuite, default_branch: &str) -> String {
    let mut conditions = Vec::new();
    if suite.events.contains(&MiseTaskSuiteEvent::Push) {
        conditions.push(format!(
            "github.event_name == 'push' && github.ref == 'refs/heads/{default_branch}'"
        ));
    }
    if suite.events.contains(&MiseTaskSuiteEvent::Schedule) {
        let crons = suite
            .schedule
            .iter()
            .map(|cron| format!("github.event.schedule == '{cron}'"))
            .collect::<Vec<_>>();
        conditions.push(format!(
            "github.event_name == 'schedule' && ({})",
            crons.join(" || ")
        ));
    }
    conditions.join(" || ")
}

fn triggers_yaml(suites: &[&MiseTaskSuitePolicy], default_branch: &str) -> Yaml {
    let has_push = suites
        .iter()
        .any(|policy| policy.suite.events.contains(&MiseTaskSuiteEvent::Push));
    let mut crons = BTreeSet::new();
    for policy in suites {
        if policy.suite.events.contains(&MiseTaskSuiteEvent::Schedule) {
            crons.extend(policy.suite.schedule.iter().cloned());
        }
    }
    let mut events = Vec::new();
    if has_push {
        events.push((
            "push".to_owned(),
            Yaml::Map(vec![(
                "branches".to_owned(),
                Yaml::Seq(vec![Yaml::str(default_branch.to_owned())]),
            )]),
        ));
    }
    if !crons.is_empty() {
        events.push((
            "schedule".to_owned(),
            Yaml::Seq(
                crons
                    .into_iter()
                    .map(|cron| Yaml::Map(vec![("cron".to_owned(), Yaml::str(cron))]))
                    .collect(),
            ),
        ));
    }
    Yaml::Map(events)
}

fn artifact_step(
    policy: &MiseTaskSuitePolicy,
    artifact: &RustSuiteArtifact,
) -> Result<Yaml, RenderError> {
    let Some(path) = artifact.paths.first() else {
        return Err(RenderError::InvalidWorkflow(
            "task_suite_artifact_path".to_owned(),
        ));
    };
    crate::artifact_paths::check_artifact_path(path)?;
    let mut step = steps::action_step(
        "Upload task suite artifact",
        steps::UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "name".to_owned(),
                velnor_actions_contract::mise_task_suite_artifact_name(
                    &policy.suite.id,
                    &artifact.name,
                ),
            ),
            ("path".to_owned(), path.clone()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                artifact.retention_days.to_string(),
            ),
        ]),
    )?;
    step.condition = Some(artifact.when.github_condition().to_owned());
    job_step_to_yaml(policy, &step)
}

#[cfg(test)]
#[path = "mise_task_suites_tests.rs"]
mod tests;
