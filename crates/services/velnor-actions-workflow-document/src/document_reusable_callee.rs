//! Fixed YAML document for the V1 reusable-workflow callee.

use velnor_actions_contract_workflow::workflow::reusable_callee::{
    REUSABLE_CALLEE_EVENT, REUSABLE_CALLEE_GUARD_JOB, REUSABLE_CALLEE_INPUT_TYPE,
    REUSABLE_CALLEE_INPUTS, REUSABLE_CALLEE_WRITE_JOB, ReusableCalleePolicy,
    ReusableCalleePolicyError,
};
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::yaml::Yaml;

/// Build the strict reusable-callee workflow document.
///
/// The six workflow inputs are data only. Caller identity is enforced by the
/// guard condition, the source SHA is checked dynamically, and the later
/// publisher job is fail-closed until V2 supplies its runtime.
///
/// # Errors
///
/// Returns [`RenderError::PolicyRejected`] when typed policy is ambiguous.
pub fn reusable_callee_document(policy: &ReusableCalleePolicy) -> Result<Yaml, RenderError> {
    policy
        .validate()
        .map_err(|error| RenderError::PolicyRejected {
            policy: "reusable_callee_v1".to_owned(),
            problem: policy_problem(error).to_owned(),
        })?;
    let caller_ref = format!("refs/heads/{}", policy.caller_branch);
    let workflow_ref = format!(
        "{}/{}@{caller_ref}",
        policy.caller_repository, policy.caller_workflow_path
    );
    let guard_condition = format!(
        "github.event_name == '{REUSABLE_CALLEE_EVENT}' \
         && github.repository == '{}' \
         && github.ref == '{caller_ref}' \
         && github.workflow_ref == '{workflow_ref}' \
         && github.workflow_sha == github.sha \
         && job.workflow_repository == '{}'",
        policy.caller_repository, policy.callee_repository
    );
    let jobs = vec![
        (
            REUSABLE_CALLEE_GUARD_JOB.to_owned(),
            guard_job(&guard_condition),
        ),
        (REUSABLE_CALLEE_WRITE_JOB.to_owned(), write_job()),
    ];
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Velnor trusted release publisher"),
        ),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "workflow_call".to_owned(),
                Yaml::Map(vec![("inputs".to_owned(), inputs_yaml())]),
            )]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]))
}

fn inputs_yaml() -> Yaml {
    Yaml::Map(
        REUSABLE_CALLEE_INPUTS
            .iter()
            .map(|input| {
                (
                    input.name.to_owned(),
                    Yaml::Map(vec![
                        ("type".to_owned(), Yaml::str(REUSABLE_CALLEE_INPUT_TYPE)),
                        ("required".to_owned(), Yaml::Bool(true)),
                    ]),
                )
            })
            .collect(),
    )
}

fn guard_job(condition: &str) -> Yaml {
    Yaml::Map(vec![
        ("runs-on".to_owned(), Yaml::str("ubuntu-24.04")),
        ("timeout-minutes".to_owned(), Yaml::Int(5)),
        ("permissions".to_owned(), Yaml::Map(Vec::new())),
        ("if".to_owned(), Yaml::str(condition)),
        ("steps".to_owned(), Yaml::Seq(vec![guard_step()])),
    ])
}

fn write_job() -> Yaml {
    Yaml::Map(vec![
        ("runs-on".to_owned(), Yaml::str("ubuntu-24.04")),
        ("timeout-minutes".to_owned(), Yaml::Int(5)),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("actions".to_owned(), Yaml::str("read")),
                ("contents".to_owned(), Yaml::str("write")),
                ("id-token".to_owned(), Yaml::str("none")),
                ("pull-requests".to_owned(), Yaml::str("none")),
            ]),
        ),
        (
            "needs".to_owned(),
            Yaml::Seq(vec![Yaml::str(REUSABLE_CALLEE_GUARD_JOB)]),
        ),
        (
            "if".to_owned(),
            Yaml::str(format!(
                "needs.{REUSABLE_CALLEE_GUARD_JOB}.result == 'success'"
            )),
        ),
        ("steps".to_owned(), Yaml::Seq(vec![write_step()])),
    ])
}

fn guard_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Confirm V1 identity policy")),
        ("run".to_owned(), Yaml::str("exit 0")),
    ])
}

const fn policy_problem(error: ReusableCalleePolicyError) -> &'static str {
    match error {
        ReusableCalleePolicyError::Repository => "repository",
        ReusableCalleePolicyError::WorkflowPath => "workflow_path",
        ReusableCalleePolicyError::Branch => "branch",
    }
}

fn write_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("V2 runtime is not enabled")),
        ("run".to_owned(), Yaml::str("exit 108")),
    ])
}
