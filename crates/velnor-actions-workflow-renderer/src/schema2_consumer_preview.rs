use super::{
    MiseSetup,
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

#[path = "schema2_consumer_preview_trigger.rs"]
mod preview_trigger;
#[path = "schema2_consumer_preview_production.rs"]
pub(super) mod production;
#[path = "schema2_consumer_publication_budget.rs"]
mod publication_budget;
#[path = "schema2_consumer_preview_source.rs"]
mod source;
#[path = "schema2_consumer_preview_steps.rs"]
pub(super) mod steps;
#[path = "schema2_consumer_preview_tap.rs"]
pub(super) mod tap;
use steps::{Artifact, needs, operation, permissions, setup, transfer};
pub use tap::ConsumerReleaseTapSpec;

#[path = "schema2_consumer_release_admission.rs"]
mod admission;
use admission::{aggregate_admission, finish_policy_job, policy_admission};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerReleaseSpec {
    pub generator_version: String,
    pub checkout_lfs: bool,
    pub policy: PackageReleasePolicy,
    pub producer_runner: VerificationRunner,
    pub producer_timeout_minutes: u16,
    pub publication_timeout_minutes: JobTimeout,
    pub linux_acquire: Step,
    pub producer_acquire: Step,
    pub producer_acquire_by_target: BTreeMap<velnor_actions_contract::targets::ReleaseTarget, Step>,
    pub producer_setup: MiseSetup,
    pub producer_setup_by_target:
        BTreeMap<velnor_actions_contract::targets::ReleaseTarget, MiseSetup>,
    pub linux_setup: MiseSetup,
    pub install_gh_argv: Vec<String>,
    pub install_rust_argv: Vec<String>,
    pub verify_rust_argv: Vec<String>,
    pub producer_source_prep: Vec<Step>,
    pub producer_cargo_env: BTreeMap<String, String>,
    pub gh_argv: Vec<String>,
    pub homebrew_tap: Option<ConsumerReleaseTapSpec>,
    pub stable: Option<super::consumer_binary_release::ConsumerStableReleaseSpec>,
}

pub fn render_consumer_preview(spec: &ConsumerReleaseSpec) -> Result<RenderedFile, RenderError> {
    spec.policy
        .validate()
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    spec.linux_setup.validate()?;
    spec.producer_setup.validate()?;
    spec.publication_timeout_minutes
        .validate()
        .map_err(RenderError::Contract)?;
    if spec.publication_timeout_minutes.minutes()
        > velnor_actions_contract::config::MAX_PUBLICATION_RUNTIME_MINUTES
    {
        return Err(RenderError::InvalidWorkflow(
            "publication_budget_exceeds_job_limit".to_owned(),
        ));
    }
    if spec.policy.channel != PackageReleaseChannel::Preview
        || spec.policy.default_branch != "main"
        || !(1..=velnor_actions_contract::config::RustBinaryReleaseProducer::MAX_TIMEOUT_MINUTES)
            .contains(&spec.producer_timeout_minutes)
    {
        return Err(RenderError::InvalidWorkflow(
            "preview_spec_invalid".to_owned(),
        ));
    }
    if spec.stable.is_some() {
        return Err(RenderError::InvalidWorkflow(
            "preview_policy_invalid".to_owned(),
        ));
    }
    let mut jobs = vec![source::job(spec)?];
    jobs.extend(production::jobs(spec)?);
    jobs.extend([signer(spec)?, publisher(spec)?, recovery(spec)?]);
    if let Some(tap) = &spec.homebrew_tap {
        jobs.extend(tap::jobs(spec, tap, TapInvocationContext::Preview)?);
    }
    let document = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(spec.policy.channel.workflow_name()),
        ),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                preview_trigger::trigger(&spec.policy.default_branch),
                ("workflow_dispatch".to_owned(), steps::recovery_inputs()),
            ]),
        ),
        ("permissions".to_owned(), Yaml::Map(Vec::new())),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str(
                        "native-preview-${{ github.repository }}-${{ github.event.workflow_run.head_sha || inputs.original_run_id }}",
                    ),
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
    spec: &ConsumerReleaseSpec,
    document: &Yaml,
) -> Result<String, RenderError> {
    preview_trigger::validate(document, steps::recovery_inputs())?;
    crate::render::fallback::render_checked_workflow(
        PackageReleaseChannel::Preview.workflow_path(),
        document,
        &spec.generator_version,
    )
}

pub(super) fn acquire(step: &Step, job: &str, runner: &str) -> Result<Yaml, RenderError> {
    non_internal_step_to_yaml(job, step, &BTreeMap::new(), runner)
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
        preview_attestation(spec),
        transfer(spec, false, Artifact::Signed),
    ];
    job_steps.insert(3, policy_admission(spec)?);
    finish_policy_job(&id, fields, job_steps)
}

fn preview_attestation(spec: &ConsumerReleaseSpec) -> Yaml {
    let mut step = steps::attest(&spec.policy);
    if let Yaml::Map(fields) = &mut step
        && let Some((_, Yaml::Map(with))) = fields.iter_mut().find(|(key, _)| key == "with")
        && let Some((_, subject_path)) = with.iter_mut().find(|(key, _)| key == "subject-path")
    {
        *subject_path = Yaml::str("${{ steps.release.outputs.attestation_subject_paths }}");
    }
    step
}

pub(super) fn publisher(spec: &ConsumerReleaseSpec) -> Result<(String, Yaml), RenderError> {
    let context = TapInvocationContext::for_channel(spec.policy.channel);
    let id = format!("{}-publisher", context.prefix());
    let mut fields = base(
        "Publish authenticated channel release",
        Yaml::str("ubuntu-26.04"),
        spec.publication_timeout_minutes,
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
    let preparation_step_count = job_steps.len() - 1;
    publication_budget::apply_preparation_budget(
        &mut fields,
        &mut job_steps[..preparation_step_count],
        spec.publication_timeout_minutes,
    )?;
    finish_policy_job(&id, fields, job_steps)
}

pub(super) fn recovery(spec: &ConsumerReleaseSpec) -> Result<(String, Yaml), RenderError> {
    let context = match spec.policy.channel {
        PackageReleaseChannel::Preview => TapInvocationContext::Preview,
        PackageReleaseChannel::Stable => TapInvocationContext::StableAuthority,
    };
    let id = format!("{}-recovery", context.prefix());
    let mut fields = base(
        "Recover the original signed release without rebuilding",
        Yaml::str("ubuntu-26.04"),
        spec.publication_timeout_minutes,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str(match spec.policy.channel { PackageReleaseChannel::Preview => "${{ github.event_name == 'workflow_dispatch' }}", PackageReleaseChannel::Stable => "${{ github.event_name == 'workflow_dispatch' && inputs.operation == 'release' && inputs.original_run_id != '' && inputs.original_run_attempt != '' }}" }),
    ));
    fields.push((
        "needs".to_owned(),
        needs(&[&format!("{}-eligibility", context.prefix())]),
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
        operation(spec, "recover-package-release-v1", true, context)?,
    ];
    let preparation_step_count = job_steps.len() - 1;
    publication_budget::apply_preparation_budget(
        &mut fields,
        &mut job_steps[..preparation_step_count],
        spec.publication_timeout_minutes,
    )?;
    Ok(finish(&id, fields, job_steps))
}
