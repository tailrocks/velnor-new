//! Unprivileged tap preparation and independently authenticated PR write/merge jobs.

use super::{
    ConsumerPreviewSpec, MiseSetup, RenderError, Step, TapInvocationContext, Yaml, acquire, base,
    finish, run_step, steps,
};
use crate::commands::join_argv_for_run;
use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::config::{CheckExecutor, HomebrewTapConfig};

#[path = "schema2_consumer_preview_tap_operation.rs"]
mod native_operation;
use native_operation::operation;

/// Static downstream policy and official helper placement for the updater.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerPreviewTapSpec {
    /// Closed native promotion policy, never executable workflow data.
    pub policy: HomebrewTapConfig,
    /// Official helper matching the declared updater platform.
    pub updater_acquire: Step,
    /// Velnor-owned Mise bootstrap matching the updater platform.
    pub updater_setup: MiseSetup,
}

pub(super) fn jobs(
    spec: &ConsumerPreviewSpec,
    tap: &ConsumerPreviewTapSpec,
    context: TapInvocationContext,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    tap.policy
        .validate("workflow")
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    tap.updater_setup.validate()?;
    if !tap.policy.enabled || tap.policy.updater.runner.executor != CheckExecutor::Hosted {
        return Err(RenderError::InvalidWorkflow(
            "tap_updater_requires_hosted_runner".to_owned(),
        ));
    }
    Ok(vec![
        prepare(spec, tap, context)?,
        writer(spec, tap, false, context)?,
        writer(spec, tap, true, context)?,
    ])
}

fn prepare(
    spec: &ConsumerPreviewSpec,
    tap: &ConsumerPreviewTapSpec,
    context: TapInvocationContext,
) -> Result<(String, Yaml), RenderError> {
    let id = "preview-tap-prepare";
    let runner = &tap.policy.updater.runner.label;
    let mut fields = base(
        "Prepare a literal-only tap update without write credentials",
        Yaml::str(runner),
        JobTimeout::try_from(tap.policy.updater.timeout_minutes).map_err(RenderError::Contract)?,
    );
    fields.extend(prepare_bindings(context));
    let mut job_steps = vec![
        steps::setup(&tap.updater_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        acquire(&tap.updater_acquire, id, runner)?,
    ];
    if matches!(context, TapInvocationContext::Preview) {
        job_steps.push(push_download());
    }
    job_steps.push(operation(
        spec,
        tap,
        "admit-homebrew-tap-v1",
        false,
        context,
    )?);
    job_steps.push(steps::checkout(context.source()));
    job_steps.push(operation(
        spec,
        tap,
        "prepare-homebrew-tap-v1",
        false,
        context,
    )?);
    job_steps.push(steps::transfer(false, steps::Artifact::TapCandidate));
    Ok(finish(id, fields, job_steps))
}

fn writer(
    spec: &ConsumerPreviewSpec,
    tap: &ConsumerPreviewTapSpec,
    merge: bool,
    context: TapInvocationContext,
) -> Result<(String, Yaml), RenderError> {
    let (id, name, dependency, op, timeout) = if merge {
        (
            "preview-tap-merge",
            "Merge only the validated and fully reviewed PR head",
            "preview-tap-writer",
            "merge-homebrew-tap-pr-v1",
            JobTimeout::DEFAULT,
        )
    } else {
        (
            "preview-tap-writer",
            "Independently validate and create a signed-off tap PR",
            "preview-tap-prepare",
            "write-homebrew-tap-pr-v1",
            JobTimeout::DEFAULT,
        )
    };
    let mut fields = base(name, Yaml::str("ubuntu-26.04"), timeout);
    fields.extend([
        (
            "needs".to_owned(),
            steps::needs(&[context.selection(), dependency]),
        ),
        ("permissions".to_owned(), source_permissions()),
        ("environment".to_owned(), Yaml::str("tap-update")),
    ]);
    let mut job_steps = vec![
        steps::setup(&spec.linux_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        acquire(&spec.linux_acquire, id, "ubuntu-26.04")?,
        steps::transfer(true, steps::Artifact::TapCandidate),
    ];
    if matches!(context, TapInvocationContext::Preview) {
        job_steps.insert(3, push_download());
    }
    if merge {
        job_steps.push(steps::transfer(true, steps::Artifact::TapPr));
    }
    job_steps.push(mint(tap)?);
    if merge {
        job_steps.push(mint_observer(tap)?);
    }
    job_steps.push(operation(spec, tap, op, merge, context)?);
    if !merge {
        job_steps.push(steps::transfer(false, steps::Artifact::TapPr));
    }
    Ok(finish(id, fields, job_steps))
}

fn source_permissions() -> Yaml {
    steps::permissions(&[
        ("contents", "read"),
        ("actions", "read"),
        ("attestations", "read"),
    ])
}

fn push_download() -> Yaml {
    let mut step = steps::transfer(true, steps::Artifact::Signed);
    if let Yaml::Map(fields) = &mut step {
        fields.push((
            "if".to_owned(),
            Yaml::str("${{ github.event_name == 'workflow_run' }}"),
        ));
    }
    step
}

fn mint(tap: &ConsumerPreviewTapSpec) -> Result<Yaml, RenderError> {
    let (owner, repository) = tap
        .policy
        .repository
        .split_once('/')
        .ok_or_else(|| RenderError::InvalidWorkflow("tap_repository_invalid".to_owned()))?;
    let inputs = vec![
        (
            "client-id".to_owned(),
            variable(&tap.policy.app_id_variable),
        ),
        (
            "private-key".to_owned(),
            Yaml::str(format!(
                "${{{{ secrets.{} }}}}",
                tap.policy.app_private_key_secret
            )),
        ),
        ("owner".to_owned(), Yaml::str(owner)),
        ("repositories".to_owned(), Yaml::str(repository)),
        ("permission-contents".to_owned(), Yaml::str("write")),
        ("permission-pull-requests".to_owned(), Yaml::str("write")),
        ("skip-token-revoke".to_owned(), Yaml::Bool(false)),
    ];
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Mint a short-lived token scoped to the single tap repository"),
        ),
        ("id".to_owned(), Yaml::str("tap-token")),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::CREATE_GITHUB_APP_TOKEN_USES),
        ),
        ("with".to_owned(), Yaml::Map(inputs)),
    ]))
}

fn mint_observer(tap: &ConsumerPreviewTapSpec) -> Result<Yaml, RenderError> {
    let (owner, repository) = tap
        .policy
        .repository
        .split_once('/')
        .ok_or_else(|| RenderError::InvalidWorkflow("tap_repository_invalid".to_owned()))?;
    let observer = &tap.policy.protection_observer;
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Mint a separate read-only tap protection observer"),
        ),
        ("id".to_owned(), Yaml::str("tap-protection-token")),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::CREATE_GITHUB_APP_TOKEN_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("client-id".to_owned(), variable(&observer.app_id_variable)),
                (
                    "private-key".to_owned(),
                    Yaml::str(format!(
                        "${{{{ secrets.{} }}}}",
                        observer.app_private_key_secret
                    )),
                ),
                ("owner".to_owned(), Yaml::str(owner)),
                ("repositories".to_owned(), Yaml::str(repository)),
                ("permission-administration".to_owned(), Yaml::str("read")),
                ("skip-token-revoke".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ]))
}

fn variable(name: &str) -> Yaml {
    Yaml::str(format!("${{{{ vars.{name} }}}}"))
}

fn prepare_bindings(context: TapInvocationContext) -> Vec<(String, Yaml)> {
    let (needs, condition) = match context {
        TapInvocationContext::Preview => (
            steps::needs(&[
                "preview-eligibility",
                "preview-publisher",
                "preview-recovery",
            ]),
            "${{ always() && !cancelled() && needs.preview-eligibility.result == 'success' && ((github.event_name == 'workflow_run' && needs.preview-publisher.result == 'success') || (github.event_name == 'workflow_dispatch' && needs.preview-recovery.result == 'success')) }}",
        ),
        TapInvocationContext::Reconcile => (
            steps::needs(&["preview-tap-selection"]),
            "${{ needs.preview-tap-selection.outputs.eligible == 'true' }}",
        ),
    };
    vec![
        ("needs".to_owned(), needs),
        ("if".to_owned(), Yaml::str(condition)),
        ("permissions".to_owned(), source_permissions()),
    ]
}
