//! Native preview workflow with separate source, producer, signer, and publisher jobs.

use super::{
    MiseSetup,
    consumer_release_eligibility::{ReleaseEligibilityContext, consumer_script},
    features::{base, finish, run_step},
};
use crate::{
    RenderError, commands::join_argv_for_run, document_steps::non_internal_step_to_yaml,
    render::RenderedFile, yaml::Yaml,
};
use std::collections::BTreeMap;
use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::{
    Step, config::VerificationRunner, package_release::PackageReleaseChannel,
    package_release_policy::PackageReleasePolicy,
};

#[path = "schema2_consumer_preview_context.rs"]
mod invocation;
pub(super) use invocation::TapInvocationContext;
#[path = "schema2_consumer_preview_reconcile.rs"]
mod reconcile;
pub use reconcile::render_release_tap_reconcile;

#[path = "schema2_consumer_preview_production.rs"]
pub(super) mod production;
#[path = "schema2_consumer_preview_steps.rs"]
pub(super) mod steps;
#[path = "schema2_consumer_preview_tap.rs"]
pub(super) mod tap;
use steps::{Artifact, needs, operation, outputs, permissions, setup, transfer};
pub use tap::ConsumerReleaseTapSpec;

#[path = "schema2_consumer_release_admission.rs"]
mod admission;
use admission::{aggregate_admission, finish_policy_job, policy_admission};

/// Fixed existing mutable release authority, never inferred from a receipt URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerRollingPreviewSpec {
    /// Positive existing mutable release database ID.
    pub release_id: u64,
    /// Exact legacy inventory authorization for the first committed generation.
    pub bootstrap_snapshot_sha256: Option<String>,
}

/// Generation-time, inventory-only authority for a native preview workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerReleaseSpec {
    /// Official generator version used by all isolated helpers.
    pub generator_version: String,
    /// Fetch LFS objects in every existing source checkout when explicitly enabled.
    pub checkout_lfs: bool,
    /// Closed product inventory and source repository identity.
    pub policy: PackageReleasePolicy,
    /// Producer platform selected by validated native configuration.
    pub producer_runner: VerificationRunner,
    /// Native operation time limit; producer jobs add fixed lifecycle headroom.
    pub producer_timeout_minutes: u16,
    /// Verified helper acquisition for Linux policy/signing/publication jobs.
    pub linux_acquire: Step,
    /// Verified helper acquisition for the declared producer platform.
    pub producer_acquire: Step,
    /// Verified Mise setup matching the producer platform.
    pub producer_setup: MiseSetup,
    /// Velnor-owned Mise pin used only to obtain the pinned GitHub CLI.
    pub linux_setup: MiseSetup,
    /// Fixed qualified GitHub CLI installation argv.
    pub install_gh_argv: Vec<String>,
    /// Exact pinned metadata compiler installation argv, before source discovery.
    pub install_rust_argv: Vec<String>,
    /// `sh -c` argv verifying that install file by file, before any use.
    pub verify_rust_argv: Vec<String>,
    /// Explicit P08 probe/fetch steps for every lockful workspace runtime discovers.
    pub producer_source_prep: Vec<Step>,
    /// Shared owned homes for installation, source preparation, and offline metadata.
    pub producer_cargo_env: BTreeMap<String, String>,
    /// Fixed qualified GitHub CLI argv prefix.
    pub gh_argv: Vec<String>,
    /// Independently enabled downstream PR promotion, with isolated task and App boundaries.
    pub homebrew_tap: Option<ConsumerReleaseTapSpec>,
    /// Closed manual tag coordination, present only for the stable channel.
    pub stable: Option<super::consumer_binary_release::ConsumerStableReleaseSpec>,
    /// Explicit fixed rolling target authority, absent for permanent stable.
    pub preview: Option<ConsumerRollingPreviewSpec>,
}

/// Render the independently enabled native preview workflow.
/// # Errors
/// Rejects malformed inventory, helper acquisition, pins, or generated commands.
pub fn render_consumer_preview(
    spec: &ConsumerReleaseSpec,
    routing: &crate::workflow_routing::WorkflowRoutingPolicy,
) -> Result<RenderedFile, RenderError> {
    spec.policy
        .validate()
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    spec.linux_setup.validate()?;
    spec.producer_setup.validate()?;
    if spec.policy.channel != PackageReleaseChannel::Preview
        || spec.policy.default_branch != "main"
        || !(1..=velnor_actions_contract::config::RustBinaryReleaseProducer::MAX_TIMEOUT_MINUTES)
            .contains(&spec.producer_timeout_minutes)
    {
        return Err(RenderError::InvalidWorkflow(
            "preview_spec_invalid".to_owned(),
        ));
    }
    let preview = spec
        .preview
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("rolling_policy_missing".to_owned()))?;
    if spec.stable.is_some()
        || preview.release_id == 0
        || preview
            .bootstrap_snapshot_sha256
            .as_ref()
            .is_some_and(|value| {
                value.len() != 64
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
    {
        return Err(RenderError::InvalidWorkflow(
            "rolling_policy_invalid".to_owned(),
        ));
    }
    let gate = consumer_script(
        &ReleaseEligibilityContext {
            repository: spec.policy.repository.clone(),
            default_branch: spec.policy.default_branch.clone(),
            channel: PackageReleaseChannel::Preview,
        },
        &spec.gh_argv,
        false,
    )?;
    let jobs = preview_jobs(spec, &gate)?;
    let document = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(spec.policy.channel.workflow_name()),
        ),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                super::consumer_release_eligibility::preview_trigger::trigger(
                    &spec.policy.default_branch,
                ),
                ("workflow_dispatch".to_owned(), steps::recovery_inputs()),
            ]),
        ),
        ("permissions".to_owned(), Yaml::Map(Vec::new())),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("native-preview-${{ github.repository }}"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]);
    let document = crate::workflow_routing::route_workflow_yaml(
        PackageReleaseChannel::Preview.workflow_path(),
        document,
        routing,
    )?;
    Ok(RenderedFile {
        path: PackageReleaseChannel::Preview.workflow_path().to_owned(),
        bytes: render_preview_document(spec, &document)?,
    })
}

fn preview_jobs(
    spec: &ConsumerReleaseSpec,
    gate: &str,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    let mut jobs = vec![eligibility(spec, gate)?];
    jobs.extend(production::jobs(spec)?);
    jobs.extend([signer(spec)?, publisher(spec)?, recovery(spec)?]);
    if let Some(tap) = &spec.homebrew_tap {
        jobs.extend(tap::jobs(spec, tap, TapInvocationContext::Preview)?);
    }
    Ok(jobs)
}

fn render_preview_document(
    spec: &ConsumerReleaseSpec,
    document: &Yaml,
) -> Result<String, RenderError> {
    super::consumer_release_eligibility::preview_trigger::validate(
        document,
        steps::recovery_inputs(),
    )?;
    crate::render::fallback::render_checked_workflow(
        PackageReleaseChannel::Preview.workflow_path(),
        document,
        &spec.generator_version,
    )
}

pub(super) fn acquire(step: &Step, job: &str, runner: &str) -> Result<Yaml, RenderError> {
    non_internal_step_to_yaml(job, step, &BTreeMap::new(), runner)
}

pub(super) fn eligibility(
    spec: &ConsumerReleaseSpec,
    gate: &str,
) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "Verify exact-commit Required CI",
        Yaml::str("ubuntu-26.04"),
        JobTimeout::DEFAULT,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str(match spec.policy.channel { PackageReleaseChannel::Preview => "${{ github.event_name == 'workflow_dispatch' || (github.event_name == 'workflow_run' && github.event.workflow_run.conclusion == 'success') }}", PackageReleaseChannel::Stable => "${{ github.event_name == 'workflow_dispatch' }}" }),
    ));
    fields.push((
        "permissions".to_owned(),
        permissions(&[("actions", "read"), ("contents", "read")]),
    ));
    let mut output = outputs(&[
        "source_sha",
        "workflow_authority_sha",
        "ci_run_id",
        "ci_attempt",
    ]);
    if let Yaml::Map(fields) = &mut output {
        fields.push((
            "commit_count".to_owned(),
            Yaml::str("${{ steps.history.outputs.commit_count }}"),
        ));
    }
    fields.push(("outputs".to_owned(), output));
    let mut env = vec![("GH_TOKEN", "${{ github.token }}")];
    if spec.policy.channel == PackageReleaseChannel::Stable {
        env.extend([
            ("VELNOR_STABLE_OPERATION", "${{ inputs.operation }}"),
            (
                "VELNOR_STABLE_ORIGINAL_RUN_ID",
                "${{ inputs.original_run_id }}",
            ),
            (
                "VELNOR_STABLE_ORIGINAL_RUN_ATTEMPT",
                "${{ inputs.original_run_attempt }}",
            ),
        ]);
    }
    let step = steps::script("Verify source and CI", "source", gate, &env);
    Ok(finish(
        &format!(
            "{}-eligibility",
            TapInvocationContext::for_channel(spec.policy.channel).prefix()
        ),
        fields,
        vec![
            steps::history_checkout(spec),
            setup(&spec.linux_setup),
            run_step(
                "Install pinned GitHub CLI",
                &join_argv_for_run(&spec.install_gh_argv)?,
            ),
            step,
            steps::history_count(TapInvocationContext::for_channel(spec.policy.channel)),
        ],
    ))
}

pub(super) fn signer(spec: &ConsumerReleaseSpec) -> Result<(String, Yaml), RenderError> {
    let context = TapInvocationContext::for_channel(spec.policy.channel);
    let id = format!("{}-signer", context.prefix());
    let mut fields = base(&id, Yaml::str("ubuntu-26.04"), JobTimeout::DEFAULT);
    fields.push(("if".to_owned(), Yaml::str(context.production_if())));
    fields.push((
        "needs".to_owned(),
        needs(&[
            context.selection(),
            &format!("{}-assemble", context.prefix()),
        ]),
    ));
    fields.push(("environment".to_owned(), Yaml::str(context.environment())));
    fields.push((
        "permissions".to_owned(),
        permissions(&[
            ("actions", "read"),
            ("contents", "read"),
            ("attestations", "write"),
            ("id-token", "write"),
        ]),
    ));
    let mut job_steps = vec![
        setup(&spec.linux_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        acquire(&spec.linux_acquire, &id, "ubuntu-26.04")?,
        aggregate_admission(spec)?,
        operation(
            spec,
            "sign-package-release-v1",
            true,
            TapInvocationContext::for_channel(spec.policy.channel),
        )?,
        steps::attest(&spec.policy),
        transfer(spec, false, Artifact::Signed),
    ];
    job_steps.insert(3, policy_admission(spec)?);
    finish_policy_job(&id, fields, job_steps)
}

pub(super) fn publisher(spec: &ConsumerReleaseSpec) -> Result<(String, Yaml), RenderError> {
    let context = TapInvocationContext::for_channel(spec.policy.channel);
    let id = format!("{}-publisher", context.prefix());
    let mut fields = base(
        "Publish authenticated channel release",
        Yaml::str("ubuntu-26.04"),
        JobTimeout::DEFAULT,
    );
    fields.push(("if".to_owned(), Yaml::str(context.production_if())));
    fields.push((
        "needs".to_owned(),
        needs(&[context.selection(), &format!("{}-signer", context.prefix())]),
    ));
    fields.push(("environment".to_owned(), Yaml::str(context.environment())));
    fields.push((
        "permissions".to_owned(),
        permissions(&[
            ("actions", "read"),
            ("attestations", "read"),
            ("contents", "write"),
        ]),
    ));
    let mut job_steps = vec![
        setup(&spec.linux_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        acquire(&spec.linux_acquire, &id, "ubuntu-26.04")?,
        transfer(spec, true, Artifact::Signed),
        operation(
            spec,
            "publish-package-release-v1",
            true,
            TapInvocationContext::for_channel(spec.policy.channel),
        )?,
    ];
    job_steps.insert(3, policy_admission(spec)?);
    finish_policy_job(&id, fields, job_steps)
}

#[path = "schema2_consumer_preview_recovery.rs"]
mod recovery_job;
pub(in crate::schema2) use recovery_job::recovery;

#[cfg(test)]
#[path = "schema2_consumer_preview_tests.rs"]
pub(super) mod tests;
