use crate::{RenderError, render::RenderedFile, setup::MiseSetup, yaml::Yaml};
use velnor_actions_contract::{
    Step,
    config::ReviewCompletionConfig,
    review_completion::{
        REVIEW_COMPLETION_ENVIRONMENT, REVIEW_COMPLETION_WORKFLOW_PATH, ReviewCompletionPolicy,
    },
};
#[path = "review_completion_steps.rs"]
mod steps;

#[derive(Debug)]
pub struct ReviewCompletionSpec {
    pub policy: ReviewCompletionPolicy,
    pub config: ReviewCompletionConfig,
    pub generator_version: String,
    pub setup: MiseSetup,
    pub acquire: Step,
    pub install_gh_argv: Vec<String>,
}

pub fn render_review_completion(spec: &ReviewCompletionSpec) -> Result<RenderedFile, RenderError> {
    spec.policy
        .validate()
        .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?;
    spec.config
        .validate(".velnor/config.toml")
        .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?;
    spec.setup.validate()?;
    if spec.policy.provider != spec.config.provider
        || spec.policy.check_name != spec.config.check_name
    {
        return Err(invalid());
    }
    let job = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Authenticate review completion"),
        ),
        (
            "if".to_owned(),
            Yaml::str(
                "${{ github.event_name == 'pull_request_target' || github.event.issue.pull_request }}",
            ),
        ),
        ("runs-on".to_owned(), Yaml::str("ubuntu-26.04")),
        ("timeout-minutes".to_owned(), Yaml::Int(5)),
        (
            "environment".to_owned(),
            Yaml::str(REVIEW_COMPLETION_ENVIRONMENT),
        ),
        (
            "permissions".to_owned(),
            map(&[("contents", "read"), ("pull-requests", "read")]),
        ),
        ("steps".to_owned(), Yaml::Seq(steps::render(spec)?)),
    ]);
    let document = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Native review completion")),
        ("on".to_owned(), triggers(&spec.policy.default_branch)),
        ("permissions".to_owned(), Yaml::Map(Vec::new())),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str(
                        "review-completion-${{ github.repository }}-${{ github.event.pull_request.number || github.event.issue.number }}",
                    ),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![("review-completion".to_owned(), job)]),
        ),
    ]);
    validate_triggers(&document, &spec.policy.default_branch)?;
    let bytes = crate::render::fallback::render_checked_workflow(
        REVIEW_COMPLETION_WORKFLOW_PATH,
        &document,
        &spec.generator_version,
    )?;
    Ok(RenderedFile {
        path: REVIEW_COMPLETION_WORKFLOW_PATH.to_owned(),
        bytes,
    })
}

fn map(values: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Yaml::str(*v)))
            .collect(),
    )
}

fn triggers(branch: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "issue_comment".to_owned(),
            Yaml::Map(vec![(
                "types".to_owned(),
                Yaml::Seq(vec![
                    Yaml::annotated(
                        "created",
                        "zizmor: ignore[dangerous-triggers] -- closed source-free provider adapter",
                    ),
                    Yaml::str("edited"),
                    Yaml::str("deleted"),
                ]),
            )]),
        ),
        (
            "pull_request_target".to_owned(),
            Yaml::Map(vec![
                (
                    "types".to_owned(),
                    Yaml::Seq(
                        ["opened", "synchronize", "reopened", "ready_for_review"]
                            .into_iter()
                            .map(Yaml::str)
                            .collect(),
                    ),
                ),
                ("branches".to_owned(), Yaml::Seq(vec![Yaml::str(branch)])),
            ]),
        ),
    ])
}

fn validate_triggers(document: &Yaml, branch: &str) -> Result<(), RenderError> {
    let Yaml::Map(fields) = document else {
        return Err(invalid());
    };
    let mut rows = fields.iter().filter(|(key, _)| key == "on");
    match (rows.next(), rows.next()) {
        (Some((_, actual)), None) if actual == &triggers(branch) => Ok(()),
        _ => Err(invalid()),
    }
}
fn invalid() -> RenderError {
    RenderError::InvalidWorkflow("review_completion_closed_policy".to_owned())
}
