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
    package_release_plan::PackageReleasePlan,
};

#[path = "schema2_consumer_preview_context.rs"]
mod invocation;
use invocation::TapInvocationContext;
#[path = "schema2_consumer_preview_reconcile.rs"]
mod reconcile;
pub use reconcile::render_preview_tap_reconcile;

#[path = "schema2_consumer_preview_production.rs"]
mod production;
#[path = "schema2_consumer_preview_steps.rs"]
mod steps;
#[path = "schema2_consumer_preview_tap.rs"]
mod tap;
use steps::{Artifact, needs, operation, outputs, permissions, setup, transfer};
pub use tap::ConsumerPreviewTapSpec;

/// Generation-time, inventory-only authority for a native preview workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerPreviewSpec {
    /// Official generator version used by all isolated helpers.
    pub generator_version: String,
    /// Closed product inventory and source repository identity.
    pub plan: PackageReleasePlan,
    /// Producer platform selected by validated native configuration.
    pub producer_runner: VerificationRunner,
    /// Producer task time limit.
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
    /// Explicit P08 probe/fetch steps for every lockful workspace runtime discovers.
    pub producer_source_prep: Vec<Step>,
    /// Shared owned homes for installation, source preparation, and offline metadata.
    pub producer_cargo_env: BTreeMap<String, String>,
    /// Fixed qualified GitHub CLI argv prefix.
    pub gh_argv: Vec<String>,
    /// Independently enabled downstream PR promotion, with isolated task and App boundaries.
    pub homebrew_tap: Option<ConsumerPreviewTapSpec>,
}

/// Render the independently enabled native preview workflow.
/// # Errors
/// Rejects malformed inventory, helper acquisition, pins, or generated commands.
pub fn render_consumer_preview(spec: &ConsumerPreviewSpec) -> Result<RenderedFile, RenderError> {
    spec.plan
        .validate()
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    spec.linux_setup.validate()?;
    spec.producer_setup.validate()?;
    if spec.plan.channel != PackageReleaseChannel::Preview
        || spec.plan.default_branch != "main"
        || !JobTimeout::is_valid_minutes(u32::from(spec.producer_timeout_minutes))
    {
        return Err(RenderError::InvalidWorkflow(
            "preview_spec_invalid".to_owned(),
        ));
    }
    let gate = consumer_script(
        &ReleaseEligibilityContext {
            repository: spec.plan.repository.clone(),
            default_branch: spec.plan.default_branch.clone(),
            channel: PackageReleaseChannel::Preview,
        },
        &spec.gh_argv,
        false,
    )?;
    let mut jobs = vec![eligibility(spec, &gate)?];
    jobs.extend(production::jobs(spec)?);
    jobs.extend([signer(spec)?, publisher(spec)?, recovery(spec)?]);
    if let Some(tap) = &spec.homebrew_tap {
        jobs.extend(tap::jobs(spec, tap, TapInvocationContext::Preview)?);
    }
    let document = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(spec.plan.channel.workflow_name()),
        ),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                super::consumer_release_eligibility::preview_trigger::trigger(
                    &spec.plan.default_branch,
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
    Ok(RenderedFile {
        path: PackageReleaseChannel::Preview.workflow_path().to_owned(),
        bytes: render_preview_document(spec, &document)?,
    })
}

fn render_preview_document(
    spec: &ConsumerPreviewSpec,
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

fn acquire(step: &Step, job: &str, runner: &str) -> Result<Yaml, RenderError> {
    non_internal_step_to_yaml(job, step, &BTreeMap::new(), runner)
}

fn eligibility(spec: &ConsumerPreviewSpec, gate: &str) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "Verify exact-commit Required CI",
        Yaml::str("ubuntu-26.04"),
        JobTimeout::DEFAULT,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str("${{ github.event_name == 'workflow_dispatch' || (github.event_name == 'workflow_run' && github.event.workflow_run.conclusion == 'success') }}"),
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
    let step = steps::script(
        "Verify source and CI",
        "source",
        gate,
        &[("GH_TOKEN", "${{ github.token }}")],
    );
    Ok(finish(
        "preview-eligibility",
        fields,
        vec![
            steps::history_checkout(),
            setup(&spec.linux_setup),
            run_step(
                "Install pinned GitHub CLI",
                &join_argv_for_run(&spec.install_gh_argv)?,
            ),
            step,
            steps::history_count(TapInvocationContext::Preview),
        ],
    ))
}

fn signer(spec: &ConsumerPreviewSpec) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "preview-signer",
        Yaml::str("ubuntu-26.04"),
        JobTimeout::DEFAULT,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str("${{ github.event_name == 'workflow_run' }}"),
    ));
    fields.push((
        "needs".to_owned(),
        needs(&["preview-eligibility", "preview-assemble"]),
    ));
    fields.push(("environment".to_owned(), Yaml::str("preview-publish")));
    fields.push((
        "permissions".to_owned(),
        permissions(&[
            ("actions", "read"),
            ("contents", "read"),
            ("attestations", "write"),
            ("id-token", "write"),
        ]),
    ));
    Ok(finish(
        "preview-signer",
        fields,
        vec![
            setup(&spec.linux_setup),
            run_step(
                "Install pinned GitHub CLI",
                &join_argv_for_run(&spec.install_gh_argv)?,
            ),
            acquire(&spec.linux_acquire, "preview-signer", "ubuntu-26.04")?,
            aggregate_admission(spec)?,
            operation(
                spec,
                "sign-package-release-v1",
                true,
                TapInvocationContext::Preview,
            )?,
            steps::attest(&spec.plan),
            transfer(false, Artifact::Signed),
        ],
    ))
}

fn aggregate_admission(spec: &ConsumerPreviewSpec) -> Result<Yaml, RenderError> {
    let mut step = operation(
        spec,
        "admit-preview-aggregate-v1",
        true,
        TapInvocationContext::Preview,
    )?;
    if let Yaml::Map(fields) = &mut step {
        let (_, id) = fields
            .iter_mut()
            .find(|(key, _)| key == "id")
            .ok_or_else(|| {
                RenderError::InvalidWorkflow("preview_operation_id_missing".to_owned())
            })?;
        *id = Yaml::str("aggregate");
    }
    production::operation_env(
        &mut step,
        &[
            (
                "VELNOR_PREVIEW_AGGREGATE_ARTIFACT_ID",
                "${{ needs.preview-assemble.outputs.artifact_id }}",
            ),
            (
                "VELNOR_PREVIEW_AGGREGATE_ARTIFACT_DIGEST",
                "${{ needs.preview-assemble.outputs.artifact_digest }}",
            ),
        ],
    )?;
    Ok(step)
}

fn publisher(spec: &ConsumerPreviewSpec) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "Publish immutable authenticated preview",
        Yaml::str("ubuntu-26.04"),
        JobTimeout::DEFAULT,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str("${{ github.event_name == 'workflow_run' }}"),
    ));
    fields.push((
        "needs".to_owned(),
        needs(&["preview-eligibility", "preview-signer"]),
    ));
    fields.push(("environment".to_owned(), Yaml::str("preview-publish")));
    fields.push((
        "permissions".to_owned(),
        permissions(&[
            ("actions", "read"),
            ("attestations", "read"),
            ("contents", "write"),
        ]),
    ));
    Ok(finish(
        "preview-publisher",
        fields,
        vec![
            setup(&spec.linux_setup),
            run_step(
                "Install pinned GitHub CLI",
                &join_argv_for_run(&spec.install_gh_argv)?,
            ),
            acquire(&spec.linux_acquire, "preview-publisher", "ubuntu-26.04")?,
            transfer(true, Artifact::Signed),
            operation(
                spec,
                "publish-package-release-v1",
                true,
                TapInvocationContext::Preview,
            )?,
        ],
    ))
}

fn recovery(spec: &ConsumerPreviewSpec) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "Recover original signed preview without rebuilding",
        Yaml::str("ubuntu-26.04"),
        JobTimeout::DEFAULT,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str("${{ github.event_name == 'workflow_dispatch' }}"),
    ));
    fields.push(("needs".to_owned(), needs(&["preview-eligibility"])));
    fields.push(("environment".to_owned(), Yaml::str("preview-publish")));
    fields.push((
        "permissions".to_owned(),
        permissions(&[
            ("actions", "read"),
            ("attestations", "read"),
            ("contents", "write"),
        ]),
    ));
    Ok(finish(
        "preview-recovery",
        fields,
        vec![
            setup(&spec.linux_setup),
            run_step(
                "Install pinned GitHub CLI",
                &join_argv_for_run(&spec.install_gh_argv)?,
            ),
            acquire(&spec.linux_acquire, "preview-recovery", "ubuntu-26.04")?,
            operation(
                spec,
                "recover-package-release-v1",
                true,
                TapInvocationContext::Preview,
            )?,
        ],
    ))
}

#[cfg(test)]
#[path = "schema2_consumer_preview_tests.rs"]
mod tests;
