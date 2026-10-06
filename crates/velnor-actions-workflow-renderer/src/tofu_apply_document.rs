//! Document assembly for the protected `OpenTofu` apply workflow.

use crate::RenderError;
use crate::setup::mise_setup_step;
use crate::steps_plain::plain_step_to_yaml;
use crate::tofu_apply::{
    TOFU_APPLY_CONCURRENCY_GROUP, TOFU_APPLY_JOB_ID, TOFU_APPLY_TIMEOUT_MINUTES,
    TOFU_APPLY_WORKFLOW_NAME, TofuApplySpec,
};
use crate::tofu_apply_steps::{
    aws_credentials_step, branch_head_guard_step, checkout_yaml, install_opentofu_step,
    required_tokens_step, tofu_apply_step, tofu_backend_validation_step, tofu_cleanup_step,
    tofu_init_step, tofu_live_verify_step, tofu_plan_review_step, tofu_plan_step,
};
use crate::yaml::Yaml;

/// Build the ordered, least-privilege steps for the apply job.
fn apply_steps(spec: &TofuApplySpec) -> Result<Vec<Yaml>, RenderError> {
    let config = &spec.config;
    let checkout = checkout_yaml(&spec.checkout_uses)?;
    let reject_stale =
        branch_head_guard_step("Reject stale default-branch revision", &spec.default_branch)?;
    let mise = plain_step_to_yaml(&mise_setup_step(&spec.mise_setup)?)?;
    let check_tokens = required_tokens_step(config)?;
    let aws = aws_credentials_step(config)?;
    let install = install_opentofu_step(&spec.opentofu_version)?;
    let init = tofu_init_step(spec)?;
    let backend = tofu_backend_validation_step(spec)?;
    let before_plan = branch_head_guard_step(
        "Recheck default-branch head before planning",
        &spec.default_branch,
    )?;
    let plan = tofu_plan_step(spec)?;
    let review = tofu_plan_review_step(spec)?;
    let before_apply = branch_head_guard_step(
        "Recheck default-branch head before apply",
        &spec.default_branch,
    )?;
    let apply = tofu_apply_step(spec)?;
    let verify = tofu_live_verify_step(spec)?;
    let cleanup = tofu_cleanup_step()?;

    Ok(vec![
        checkout,
        reject_stale,
        mise,
        check_tokens,
        aws,
        install,
        init,
        backend,
        before_plan,
        plan,
        review,
        before_apply,
        apply,
        verify,
        cleanup,
    ])
}

/// Build the single privileged job with credentials scoped to apply steps.
fn apply_job(spec: &TofuApplySpec, steps: Vec<Yaml>) -> Yaml {
    let config = &spec.config;
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(TOFU_APPLY_WORKFLOW_NAME)),
        (
            "if".to_owned(),
            Yaml::str("${{ github.ref_protected == true }}"),
        ),
        ("runs-on".to_owned(), Yaml::str(spec.runs_on.clone())),
        (
            "environment".to_owned(),
            Yaml::str(config.environment.clone()),
        ),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(TOFU_APPLY_TIMEOUT_MINUTES),
        ),
        (
            "env".to_owned(),
            Yaml::Map(
                crate::toolchain_env::job_level_env()
                    .into_iter()
                    .map(|(key, value)| (key, Yaml::str(value)))
                    .collect(),
            ),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("contents".to_owned(), Yaml::str("read")),
                ("id-token".to_owned(), Yaml::str("write")),
            ]),
        ),
        (
            "defaults".to_owned(),
            Yaml::Map(vec![(
                "run".to_owned(),
                Yaml::Map(vec![
                    (
                        "shell".to_owned(),
                        Yaml::str("bash --noprofile --norc -euo pipefail {0}"),
                    ),
                    (
                        "working-directory".to_owned(),
                        Yaml::str(config.root.as_str().to_owned()),
                    ),
                ]),
            )]),
        ),
        ("steps".to_owned(), Yaml::Seq(steps)),
    ])
}

/// Wrap the apply job in the protected-default-branch workflow document.
fn workflow_document(spec: &TofuApplySpec, job: Yaml) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(TOFU_APPLY_WORKFLOW_NAME)),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "push".to_owned(),
                Yaml::Map(vec![(
                    "branches".to_owned(),
                    Yaml::Seq(vec![Yaml::str(spec.default_branch.clone())]),
                )]),
            )]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("none"))]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                ("group".to_owned(), Yaml::str(TOFU_APPLY_CONCURRENCY_GROUP)),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![(TOFU_APPLY_JOB_ID.to_owned(), job)]),
        ),
    ])
}

/// Build the single job with credentials scoped only to the steps that need them.
pub(super) fn tofu_apply_document(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
    let steps = apply_steps(spec)?;
    let job = apply_job(spec, steps);
    Ok(workflow_document(spec, job))
}
