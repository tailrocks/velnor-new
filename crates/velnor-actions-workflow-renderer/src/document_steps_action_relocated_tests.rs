use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, yaml::Yaml};

use super::{ActionStepContext, action_step_to_yaml};

const QUALIFICATION_PREPARE: &str =
    "./.github/actions/qualification-cache-prepare-rust-velnor-actions-cli";
const SOURCE_CAMPAIGN: &str = "${{ needs.plan.outputs.qualification_campaign }}";
const RELOCATED_CAMPAIGN: &str = "${{ inputs.qualification_campaign }}";
const CACHE_RESTORE: &str = crate::cache_steps::TOOLS_RESTORE_ACTION_USES;

#[test]
fn qualification_campaign_input_alias_is_exact_and_child_scoped() {
    assert!(
        render(
            QUALIFICATION_PREPARE,
            "qualification_campaign",
            SOURCE_CAMPAIGN,
            false
        )
        .is_ok()
    );
    assert!(
        render(
            QUALIFICATION_PREPARE,
            "qualification_campaign",
            SOURCE_CAMPAIGN,
            true
        )
        .is_ok()
    );
    assert!(
        render(
            QUALIFICATION_PREPARE,
            "qualification_campaign",
            RELOCATED_CAMPAIGN,
            true
        )
        .is_ok()
    );
    assert!(
        render(
            QUALIFICATION_PREPARE,
            "qualification_campaign",
            RELOCATED_CAMPAIGN,
            false
        )
        .is_err()
    );

    for (uses, key, value) in [
        (
            QUALIFICATION_PREPARE,
            "qualification_campaign",
            "${{ inputs.plan_result }}",
        ),
        (
            QUALIFICATION_PREPARE,
            "qualification_campaign",
            "${{ inputs.qualification_campaign }} || true",
        ),
        (QUALIFICATION_PREPARE, "other", RELOCATED_CAMPAIGN),
        (CACHE_RESTORE, "qualification_campaign", RELOCATED_CAMPAIGN),
    ] {
        assert!(
            render(uses, key, value, true).is_err(),
            "accepted unsupported composite input: {uses} / {key} / {value}"
        );
    }
}

#[test]
fn relocated_tools_restore_accepts_alias_only_in_child_scope() -> Result<(), RenderError> {
    let step = crate::cache_steps::tools_cache_restore_step(
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION,
        &"a".repeat(64),
        Some(crate::cache_p08::TOOLS_CACHE_RELOCATED_RESTORE_CONDITION.to_owned()),
    )?;
    assert!(render_step(&step, true).is_ok());
    assert!(render_step(&step, false).is_err());
    Ok(())
}

fn render(uses: &str, key: &str, value: &str, relocated_child: bool) -> Result<Yaml, RenderError> {
    let with = BTreeMap::from([(key.to_owned(), value.to_owned())]);
    let env = BTreeMap::new();
    let step = Step {
        name: "Prepare qualification cache".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: with.clone(),
            env: env.clone(),
        },
    };
    render_action(&step, uses, &with, &env, relocated_child)
}

fn render_step(step: &Step, relocated_child: bool) -> Result<Yaml, RenderError> {
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "relocated_action_test_kind".to_owned(),
        ));
    };
    render_action(step, uses, with, env, relocated_child)
}

fn render_action(
    step: &Step,
    uses: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
    relocated_child: bool,
) -> Result<Yaml, RenderError> {
    let job_env = BTreeMap::new();
    action_step_to_yaml(
        step,
        uses,
        with,
        env,
        ActionStepContext {
            job_id: "rust-velnor-actions-cli__hosted",
            job_env: &job_env,
            runs_on: Some("ubuntu-26.04"),
            composite: false,
            relocated_child,
        },
    )
}
