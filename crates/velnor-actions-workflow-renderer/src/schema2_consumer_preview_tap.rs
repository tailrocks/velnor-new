//! Unprivileged tap preparation and independently authenticated PR write/merge jobs.

use super::{
    ConsumerPreviewSpec, MiseSetup, RenderError, Step, Yaml, acquire, base, finish, run_step, steps,
};
use crate::commands::join_argv_for_run;
use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::{
    canonical_json_bytes,
    config::{CheckExecutor, HomebrewTapConfig},
};

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
        prepare(spec, tap)?,
        writer(spec, tap, false)?,
        writer(spec, tap, true)?,
    ])
}

fn prepare(
    spec: &ConsumerPreviewSpec,
    tap: &ConsumerPreviewTapSpec,
) -> Result<(String, Yaml), RenderError> {
    let id = "preview-tap-prepare";
    let runner = &tap.policy.updater.runner.label;
    let mut fields = base(
        "Prepare a literal-only tap update without write credentials",
        Yaml::str(runner),
        JobTimeout::try_from(tap.policy.updater.timeout_minutes).map_err(RenderError::Contract)?,
    );
    fields.extend([
        ("needs".to_owned(), steps::needs(&["preview-eligibility", "preview-publisher", "preview-recovery"])),
        ("if".to_owned(), Yaml::str("${{ always() && !cancelled() && needs.preview-eligibility.result == 'success' && ((github.event_name == 'push' && needs.preview-publisher.result == 'success') || (github.event_name == 'workflow_dispatch' && needs.preview-recovery.result == 'success')) }}")),
        ("permissions".to_owned(), source_permissions()),
    ]);
    let download = push_download();
    Ok(finish(
        id,
        fields,
        vec![
            steps::checkout("${{ needs.preview-eligibility.outputs.source_sha }}"),
            steps::setup(&tap.updater_setup),
            run_step(
                "Install pinned GitHub CLI",
                &join_argv_for_run(&spec.install_gh_argv)?,
            ),
            acquire(&tap.updater_acquire, id, runner)?,
            download,
            operation(spec, tap, "prepare-homebrew-tap-v1", false)?,
            steps::transfer(false, steps::Artifact::TapCandidate),
        ],
    ))
}

fn writer(
    spec: &ConsumerPreviewSpec,
    tap: &ConsumerPreviewTapSpec,
    merge: bool,
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
            steps::needs(&["preview-eligibility", dependency]),
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
        push_download(),
        steps::transfer(true, steps::Artifact::TapCandidate),
    ];
    if merge {
        job_steps.push(steps::transfer(true, steps::Artifact::TapPr));
    }
    job_steps.push(mint(tap, merge)?);
    job_steps.push(operation(spec, tap, op, merge)?);
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
            Yaml::str("${{ github.event_name == 'push' }}"),
        ));
    }
    step
}

fn mint(tap: &ConsumerPreviewTapSpec, merge: bool) -> Result<Yaml, RenderError> {
    let (owner, repository) = tap
        .policy
        .repository
        .split_once('/')
        .ok_or_else(|| RenderError::InvalidWorkflow("tap_repository_invalid".to_owned()))?;
    let mut inputs = vec![
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
    if merge {
        inputs.extend([
            ("permission-checks".to_owned(), Yaml::str("read")),
            ("permission-administration".to_owned(), Yaml::str("read")),
        ]);
    }
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

fn operation(
    spec: &ConsumerPreviewSpec,
    tap: &ConsumerPreviewTapSpec,
    op: &str,
    merge: bool,
) -> Result<Yaml, RenderError> {
    let mut step = steps::operation(spec, op, true)?;
    let policy = String::from_utf8(
        canonical_json_bytes(&tap.policy)
            .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?,
    )
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let mut values = vec![("VELNOR_TAP_POLICY".to_owned(), Yaml::str(policy))];
    if op != "prepare-homebrew-tap-v1" {
        values.extend([
            (
                "VELNOR_TAP_APP_ID".to_owned(),
                variable(&tap.policy.app_id_variable),
            ),
            (
                "VELNOR_TAP_APP_SLUG".to_owned(),
                Yaml::str("${{ steps.tap-token.outputs.app-slug }}"),
            ),
            (
                "VELNOR_TAP_INSTALLATION_ID".to_owned(),
                Yaml::str("${{ steps.tap-token.outputs.installation-id }}"),
            ),
            (
                (if merge {
                    "VELNOR_TAP_MERGE_TOKEN"
                } else {
                    "VELNOR_TAP_TOKEN"
                })
                .to_owned(),
                Yaml::str("${{ steps.tap-token.outputs.token }}"),
            ),
        ]);
    }
    if merge {
        values.push((
            "VELNOR_TAP_COMPLETION_APP_ID".to_owned(),
            variable(&tap.policy.review_completion.app_id_variable),
        ));
        values.extend(
            tap.policy
                .review_completion
                .expected_reviewers
                .iter()
                .enumerate()
                .map(|(index, reviewer)| {
                    (
                        format!("VELNOR_TAP_REVIEWER_APP_ID_{index}"),
                        variable(&reviewer.app_id_variable),
                    )
                }),
        );
    }
    if let Yaml::Map(fields) = &mut step
        && let Some((_, Yaml::Map(env))) = fields.iter_mut().find(|(key, _)| key == "env")
    {
        env.extend(values);
    }
    Ok(step)
}

fn variable(name: &str) -> Yaml {
    Yaml::str(format!("${{{{ vars.{name} }}}}"))
}
