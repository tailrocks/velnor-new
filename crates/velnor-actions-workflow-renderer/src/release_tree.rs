// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    RenderError, guard, marker,
    release_config::{ReleasePlzConfig, render_release_plz_config},
    release_gates::{ReleaseConfigBinding, check_release_jobs},
    release_jobs::{ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec},
    release_spec::ReleaseTriggers,
    render::RenderedFile,
    steps,
    yaml::Yaml,
};

#[path = "release_rehearsal.rs"]
pub mod rehearsal;

pub const RELEASE_HOSTED_RUNNER: &str = "ubuntu-24.04";
pub const RELEASE_WORKFLOW_PATH: &str = ".github/workflows/release.yml";
pub const RELEASE_INTENT_PATH: &str = ".github/velnor-release-intent.toml";
pub const RELEASE_CONFIG_PATH: &str = ".github/release-plz.toml";
pub const RELEASE_SOURCE_DIR: &str = "release-source";
pub const RELEASE_CURRENT_PATHS: &[&str] = &[
    RELEASE_INTENT_PATH,
    RELEASE_CONFIG_PATH,
    RELEASE_WORKFLOW_PATH,
];
pub const RELEASE_TREE_PATHS: &[&str] = &[
    ".github/release-plz-bootstrap.toml",
    RELEASE_INTENT_PATH,
    ".github/release-plz-publish.toml",
    RELEASE_CONFIG_PATH,
    RELEASE_WORKFLOW_PATH,
];

#[must_use]
pub fn release_stale_paths() -> &'static [&'static str] {
    RELEASE_TREE_PATHS
}

#[derive(Debug, Clone)]
pub struct ReleaseRenderContext {
    pub generator_version: String,
    pub runs_on: String,
}

impl ReleaseRenderContext {
    pub fn validate(&self) -> Result<(), RenderError> {
        marker::validate_version(&self.generator_version)?;
        guard::validate_runs_on(&self.runs_on)
    }
}

pub fn render_release_workflow(
    spec: &ReleaseWorkflowSpec,
    ctx: &ReleaseRenderContext,
    rust_version: &str,
) -> Result<String, RenderError> {
    ctx.validate()?;
    if ctx.runs_on != RELEASE_HOSTED_RUNNER {
        return Err(RenderError::InvalidWorkflow(
            "crate_release_runner_untrusted".to_owned(),
        ));
    }
    spec.validate()?;
    for (id, job) in &spec.jobs {
        if job.runs_on != ctx.runs_on {
            return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
        }
    }
    let binding = ReleaseConfigBinding {
        generator_version: &ctx.generator_version,
        release_plz_version: &spec.release_plz_version,
        rust_version,
    };
    check_release_jobs(spec, &binding)?;
    let document = release_document(spec)?;
    let text = crate::render::fallback::render_checked_workflow(
        RELEASE_WORKFLOW_PATH,
        &document,
        &ctx.generator_version,
    )?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(text)
}

fn release_document(spec: &ReleaseWorkflowSpec) -> Result<Yaml, RenderError> {
    let mut jobs = Vec::with_capacity(spec.jobs.len());
    for (id, job) in &spec.jobs {
        jobs.push((id.clone(), release_job_to_yaml(job)?));
    }
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(spec.name.clone())),
        ("on".to_owned(), release_triggers_to_yaml(&spec.triggers)),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str(spec.concurrency.group.clone()),
                ),
                (
                    "cancel-in-progress".to_owned(),
                    Yaml::Bool(spec.concurrency.cancel_in_progress),
                ),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]))
}

fn release_triggers_to_yaml(triggers: &ReleaseTriggers) -> Yaml {
    let branches: Vec<Yaml> = triggers
        .push_branches
        .iter()
        .map(|branch| Yaml::str(branch.clone()))
        .collect();
    let mut entries = vec![(
        "push".to_owned(),
        Yaml::Map(vec![("branches".to_owned(), Yaml::Seq(branches))]),
    )];
    if let Some(schedule) = &triggers.schedule {
        let crons: Vec<Yaml> = schedule
            .cron
            .iter()
            .map(|cron| Yaml::Map(vec![("cron".to_owned(), Yaml::str(cron.clone()))]))
            .collect();
        entries.push(("schedule".to_owned(), Yaml::Seq(crons)));
    }
    let mut inputs = Vec::with_capacity(triggers.dispatch_inputs.len());
    for input in &triggers.dispatch_inputs {
        let mut fields = vec![
            (
                "description".to_owned(),
                Yaml::str(input.description.clone()),
            ),
            ("required".to_owned(), Yaml::Bool(input.required)),
        ];
        if let Some(default) = &input.default {
            fields.push(("default".to_owned(), Yaml::str(default.clone())));
        }
        inputs.push((input.name.clone(), Yaml::Map(fields)));
    }
    if !inputs.is_empty() {
        entries.push((
            "workflow_dispatch".to_owned(),
            Yaml::Map(vec![("inputs".to_owned(), Yaml::Map(inputs))]),
        ));
    }
    Yaml::Map(entries)
}

pub fn render_release_job(job: &ReleaseJobSpec) -> Result<String, RenderError> {
    Ok(crate::yaml::render_yaml(&release_job_to_yaml(job)?))
}

fn release_job_to_yaml(job: &ReleaseJobSpec) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&job.display_name)?;
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), Yaml::str(job.runs_on.clone())),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
    ];
    if let Some(environment) = &job.environment {
        entries.push(("environment".to_owned(), Yaml::str(environment.clone())));
    }
    entries.push((
        "permissions".to_owned(),
        Yaml::Map(vec![
            (
                "actions".to_owned(),
                Yaml::str(job.permissions.actions.as_str()),
            ),
            (
                "checks".to_owned(),
                Yaml::str(job.permissions.checks.as_str()),
            ),
            (
                "contents".to_owned(),
                Yaml::str(job.permissions.contents.as_str()),
            ),
            (
                "pull-requests".to_owned(),
                Yaml::str(job.permissions.pull_requests.as_str()),
            ),
            (
                "id-token".to_owned(),
                Yaml::str(job.permissions.id_token.as_str()),
            ),
        ]),
    ));
    if !job.needs.is_empty() {
        let needs: Vec<Yaml> = job
            .needs
            .iter()
            .map(|need| Yaml::str(need.clone()))
            .collect();
        entries.push(("needs".to_owned(), Yaml::Seq(needs)));
    }
    if !job.outputs.is_empty() {
        entries.push((
            "outputs".to_owned(),
            Yaml::Map(
                job.outputs
                    .iter()
                    .map(|(name, value)| (name.clone(), Yaml::str(value.clone())))
                    .collect(),
            ),
        ));
    }
    if let Some(condition) = &job.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    let mut rendered = Vec::with_capacity(job.steps.len());
    for step in &job.steps {
        rendered.push(crate::steps_plain::plain_step_to_yaml(step)?);
    }
    entries.push(("steps".to_owned(), Yaml::Seq(rendered)));
    Ok(Yaml::Map(entries))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFiles {
    pub workflow: RenderedFile,
    pub config: RenderedFile,
    pub intent: RenderedFile,
}

impl ReleaseFiles {
    #[must_use]
    pub fn as_sorted_vec(&self) -> Vec<RenderedFile> {
        let mut files = vec![
            self.workflow.clone(),
            self.config.clone(),
            self.intent.clone(),
        ];
        files.sort_by(|left, right| left.path.cmp(&right.path));
        files
    }
}

fn release_intent_mode_matches(
    spec: &ReleaseWorkflowSpec,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> bool {
    match (
        &intent.recovery,
        &intent.new_crates,
        &spec.plan.version,
        spec.plan.dispatch,
    ) {
        (None, None, None, false) => true,
        (Some(recovery), None, Some(version), true) => {
            recovery.source_sha == spec.plan.source_sha
                && &recovery.version == version
                && spec.plan.packages.get(&recovery.package) == Some(version)
        }
        (None, Some(creation), None, true) => {
            creation.source_sha == spec.plan.source_sha
                && creation
                    .packages
                    .iter()
                    .all(|(name, version)| spec.plan.packages.get(name) == Some(version))
        }
        _ => false,
    }
}

pub fn render_release_files(
    spec: &ReleaseWorkflowSpec,
    ctx: &ReleaseRenderContext,
    config: &ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<ReleaseFiles, RenderError> {
    intent.validate().map_err(RenderError::Contract)?;
    let intent_packages: std::collections::BTreeSet<_> = intent.packages.iter().collect();
    let plan_packages: std::collections::BTreeSet<_> = spec.plan.packages.keys().collect();
    let config_packages: std::collections::BTreeSet<_> = config
        .packages
        .iter()
        .map(|package| &package.name)
        .collect();
    let publisher_role = match intent.authentication {
        velnor_actions_contract::config::ReleaseAuthentication::TrustedPublishing => {
            ReleaseRole::PublishOidc
        }
        velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken => {
            ReleaseRole::PublishBootstrap
        }
    };
    if intent.new_crates != spec.new_crates
        || intent.journal_predecessor != spec.journal_predecessor
        || !release_pin_matches(spec, intent)
        || !spec.jobs.values().any(|job| job.role == publisher_role)
        || intent.generator_version != ctx.generator_version
        || intent.release_plz_version != spec.release_plz_version
        || intent.runner != ctx.runs_on
        || velnor_actions_mise::catalog::qualified_release_plz(
            &intent.release_plz_version,
            &intent.runner,
        )
        .is_none()
        || intent.repository != spec.repository
        || spec.triggers.push_branches.as_slice() != [intent.default_branch.as_str()]
        || intent.environment != spec.publish_environment
        || intent.environment != spec.bootstrap_environment
        || intent.registry != spec.plan.registry
        || intent_packages != plan_packages
        || intent_packages != config_packages
        || !release_features_match(config, intent)
        || intent.tag_template != config.tag_pattern
        || !release_intent_mode_matches(spec, intent)
    {
        return Err(RenderError::InvalidWorkflow(
            "release_intent_mismatch".to_owned(),
        ));
    }
    let workflow = render_release_workflow(spec, ctx, &intent.rust_version)?;
    let normal = render_release_plz_config(config, &ctx.generator_version)?;
    let intent_text = toml::to_string_pretty(intent)
        .map_err(|err| RenderError::InvalidWorkflow(format!("intent_toml:{err}")))?;
    let intent_text = marker::with_marker(&ctx.generator_version, &intent_text)?;
    let files = ReleaseFiles {
        workflow: RenderedFile {
            path: RELEASE_WORKFLOW_PATH.to_owned(),
            bytes: workflow,
        },
        config: RenderedFile {
            path: RELEASE_CONFIG_PATH.to_owned(),
            bytes: normal,
        },
        intent: RenderedFile {
            path: RELEASE_INTENT_PATH.to_owned(),
            bytes: intent_text,
        },
    };
    check_release_paths(&files)?;
    Ok(files)
}

fn release_features_match(
    config: &ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> bool {
    config.packages.len() == intent.publish_features.len()
        && config.packages.iter().all(|package| {
            intent.publish_features.get(&package.name) == Some(&package.publish_features)
        })
}

fn release_pin_matches(
    spec: &ReleaseWorkflowSpec,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> bool {
    spec.jobs.values().filter(|job| matches!(job.role, ReleaseRole::Preflight | ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap | ReleaseRole::Reconcile)).all(|job| {
        matches!(job.steps.first().map(|step| &step.kind), Some(velnor_actions_contract::StepKind::Shell { env, .. })
            if env.get("VELNOR_GENERATOR_SOURCE_SHA") == Some(&intent.generator_source_sha)
            && env.get("VELNOR_GENERATOR_MANIFEST_SHA256") == Some(&intent.generator_manifest_sha256))
    })
}

fn check_release_paths(files: &ReleaseFiles) -> Result<(), RenderError> {
    guard::validate_allowlisted_path(&files.workflow.path, &[RELEASE_WORKFLOW_PATH])?;
    guard::validate_allowlisted_path(&files.config.path, &[RELEASE_CONFIG_PATH])?;
    guard::validate_allowlisted_path(&files.intent.path, &[RELEASE_INTENT_PATH])?;
    Ok(())
}
