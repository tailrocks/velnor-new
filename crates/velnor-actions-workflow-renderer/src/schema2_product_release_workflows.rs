//! Caller and reusable-workflow YAML for the product-release DAG.

use std::collections::BTreeSet;

use crate::yaml::Yaml;
use velnor_actions_contract::{DispatchInput, DispatchInputType};

use super::super::release_eligibility;
use super::{Family, HOSTED_RUNS_ON, RELEASE_CONCURRENCY};

pub(super) fn reusable_workflow_call(family: Family) -> (String, Yaml) {
    let fields = vec![
        (
            "name".to_owned(),
            Yaml::str(format!("Run {} release graph", family.label())),
        ),
        ("if".to_owned(), Yaml::str(family.selector_condition())),
        (
            "uses".to_owned(),
            Yaml::annotated(
                format!("./{}", family.workflow_path()),
                "zizmor: ignore[self-repository]",
            ),
        ),
        (
            "needs".to_owned(),
            Yaml::Seq(vec![
                Yaml::str(release_eligibility::JOB_ID),
                Yaml::str(family.prepare_id()),
            ]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("actions".to_owned(), Yaml::str("write")),
                ("artifact-metadata".to_owned(), Yaml::str("write")),
                ("attestations".to_owned(), Yaml::str("write")),
                ("contents".to_owned(), Yaml::str("write")),
                ("id-token".to_owned(), Yaml::str("write")),
            ]),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "source_sha".to_owned(),
                    Yaml::str("${{ needs.release-eligibility.outputs.source_sha }}"),
                ),
                (
                    "workflow_authority_sha".to_owned(),
                    Yaml::str("${{ needs.release-eligibility.outputs.workflow_authority_sha }}"),
                ),
                (
                    "ci_run_id".to_owned(),
                    Yaml::str("${{ needs.release-eligibility.outputs.ci_run_id }}"),
                ),
                (
                    "ci_attempt".to_owned(),
                    Yaml::str("${{ needs.release-eligibility.outputs.ci_attempt }}"),
                ),
                (
                    "release_action".to_owned(),
                    Yaml::str(format!(
                        "${{{{ needs.{}.outputs.action }}}}",
                        family.prepare_id()
                    )),
                ),
                (
                    "caller_run_id".to_owned(),
                    Yaml::str("${{ github.run_id }}"),
                ),
                (
                    "caller_attempt".to_owned(),
                    Yaml::str("${{ github.run_attempt }}"),
                ),
            ]),
        ),
    ];
    (family.call_id().to_owned(), Yaml::Map(fields))
}

pub(super) fn family_document_for_call(family: Family, jobs: Vec<(String, Yaml)>) -> Yaml {
    let mut jobs_with_authority = vec![caller_validation_job()];
    jobs_with_authority.extend(jobs);
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(format!("{} release jobs", family.label())),
        ),
        ("on".to_owned(), workflow_call_trigger()),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                (
                    "VELNOR_SOURCE_SHA".to_owned(),
                    Yaml::str("${{ inputs.source_sha }}"),
                ),
                (
                    "VELNOR_WORKFLOW_AUTHORITY_SHA".to_owned(),
                    Yaml::str("${{ inputs.workflow_authority_sha }}"),
                ),
                (
                    "VELNOR_CI_RUN_ID".to_owned(),
                    Yaml::str("${{ inputs.ci_run_id }}"),
                ),
                (
                    "VELNOR_CI_ATTEMPT".to_owned(),
                    Yaml::str("${{ inputs.ci_attempt }}"),
                ),
                (
                    "VELNOR_RELEASE_ACTION".to_owned(),
                    Yaml::str("${{ inputs.release_action }}"),
                ),
                (
                    "VELNOR_CALLER_RUN_ID".to_owned(),
                    Yaml::str("${{ inputs.caller_run_id }}"),
                ),
                (
                    "VELNOR_CALLER_ATTEMPT".to_owned(),
                    Yaml::str("${{ inputs.caller_attempt }}"),
                ),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs_with_authority)),
    ])
}

fn caller_validation_job() -> (String, Yaml) {
    let step = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Verify delegated release authority"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(DELEGATE_AUTHORITY_SCRIPT)),
    ]);
    let job = Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Verify caller release authority"),
        ),
        ("runs-on".to_owned(), Yaml::str(HOSTED_RUNS_ON)),
        ("timeout-minutes".to_owned(), Yaml::Int(5)),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("steps".to_owned(), Yaml::Seq(vec![step])),
    ]);
    ("verify-release-caller".to_owned(), job)
}

const DELEGATE_AUTHORITY_SCRIPT: &str = r#"set -euo pipefail
[[ "$GITHUB_REPOSITORY" == 'tailrocks/velnor-new' ]]
[[ "$GITHUB_REF" == 'refs/heads/main' ]]
[[ "$GITHUB_WORKFLOW_REF" == 'tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main' ]]
[[ "$GITHUB_EVENT_NAME" == 'workflow_dispatch' ]]
[[ "$GITHUB_SHA" == "$VELNOR_SOURCE_SHA" && "$GITHUB_WORKFLOW_SHA" == "$VELNOR_WORKFLOW_AUTHORITY_SHA" ]]
[[ "$GITHUB_RUN_ID" == "$VELNOR_CALLER_RUN_ID" && "$GITHUB_RUN_ATTEMPT" == "$VELNOR_CALLER_ATTEMPT" ]]
[[ "$VELNOR_SOURCE_SHA" =~ ^[0-9a-f]{40}$ && "$VELNOR_WORKFLOW_AUTHORITY_SHA" =~ ^[0-9a-f]{40}$ ]]
[[ "$VELNOR_CI_RUN_ID" =~ ^[0-9]+$ && "$VELNOR_CI_ATTEMPT" =~ ^[0-9]+$ ]]
[[ "$VELNOR_RELEASE_ACTION" == build || "$VELNOR_RELEASE_ACTION" == complete ]]"#;

fn workflow_call_trigger() -> Yaml {
    let string_input = || {
        Yaml::Map(vec![
            ("type".to_owned(), Yaml::str("string")),
            ("required".to_owned(), Yaml::Bool(true)),
        ])
    };
    Yaml::Map(vec![(
        "workflow_call".to_owned(),
        Yaml::Map(vec![(
            "inputs".to_owned(),
            Yaml::Map(
                [
                    "source_sha",
                    "workflow_authority_sha",
                    "ci_run_id",
                    "ci_attempt",
                    "release_action",
                    "caller_run_id",
                    "caller_attempt",
                ]
                .map(|name| (name.to_owned(), string_input()))
                .into(),
            ),
        )]),
    )])
}

pub(super) fn document(jobs: Vec<(String, Yaml)>, families: &[Family]) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Velnor product releases")),
        ("on".to_owned(), workflow_dispatch(families)),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                ("group".to_owned(), Yaml::str(RELEASE_CONCURRENCY)),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}

fn workflow_dispatch(families: &[Family]) -> Yaml {
    let mut choices = BTreeSet::from(["all"]);
    choices.extend(families.iter().map(|family| family.selector_value()));
    let input = DispatchInput {
        name: "release_family".to_owned(),
        required: false,
        input_type: DispatchInputType::Choice,
        choices: choices.into_iter().map(str::to_owned).collect(),
        default: Some("all".to_owned()),
    };
    let mut input_fields = vec![
        (
            "description".to_owned(),
            Yaml::str("Product release family to run; all preserves the configured release set"),
        ),
        ("type".to_owned(), Yaml::str(input.input_type.as_str())),
        ("required".to_owned(), Yaml::Bool(input.required)),
    ];
    if let Some(default) = input.default {
        input_fields.push(("default".to_owned(), Yaml::str(default)));
    }
    input_fields.push((
        "options".to_owned(),
        Yaml::Seq(
            input
                .choices
                .into_iter()
                .map(|choice| Yaml::str(choice))
                .collect(),
        ),
    ));
    Yaml::Map(vec![(
        "workflow_dispatch".to_owned(),
        Yaml::Map(vec![(
            "inputs".to_owned(),
            Yaml::Map(vec![(input.name, Yaml::Map(input_fields))]),
        )]),
    )])
}
