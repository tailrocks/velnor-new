// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use super::bun_routing::make_bun_repo;
use velnor_actions_contract::workflow::ir::WorkflowIr;
use velnor_actions_contract::{
    Job, ScaleSetSelector, StepKind, VelnorSupportWorkflow, WorkflowPolicy, expand_workflow,
};
use velnor_actions_orchestrator::{ExecutionMode, migrate_config, prepare};
use velnor_actions_workflow_renderer::{
    MiseSetup,
    render::{RenderContext, render_workflow_ir_strict},
    setup::MISE_BINARY_SHA256_LINUX_X64,
};

const HOSTED_BROWSER: &str = "bun-fixture-web--browser__hosted";
const LOCAL_BROWSER: &str = "bun-fixture-web--browser__local";
const HOSTED_REPORT: &str = "bun-fixture-web--browser--report__hosted";

struct BunRenderInput {
    ir: WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<VelnorSupportWorkflow>,
    context: RenderContext,
}

#[test]
fn schema2_bun_renderer_rejects_missing_and_extra_paired_outputs() -> Result<(), Box<dyn Error>> {
    let input = both_render_input()?;
    assert_renders(&input, &input.ir, "canonical paired fixture")?;

    let mut missing_pair = input.ir.clone();
    assert!(
        missing_pair.jobs.remove("bun-fixture-ui__local").is_some(),
        "paired fixture is missing bun-fixture-ui__local"
    );
    let required = missing_pair
        .jobs
        .get_mut("required")
        .ok_or("paired fixture is missing required job")?;
    required
        .needs
        .retain(|id| id.as_str() != "bun-fixture-ui__local");
    assert!(
        required
            .needs
            .iter()
            .all(|id| id != "bun-fixture-ui__local"),
        "missing local pair retains a required dependency"
    );
    assert_rejected(&input, &missing_pair, "missing local pair");

    let mut extra_base = input.ir.clone();
    let hosted_member = extra_base
        .jobs
        .get("bun-fixture-ui__hosted")
        .ok_or("paired fixture is missing bun-fixture-ui__hosted")?
        .clone();
    assert!(
        extra_base
            .jobs
            .insert("bun-fixture-ui".to_owned(), hosted_member)
            .is_none(),
        "paired fixture unexpectedly has unsuffixed bun-fixture-ui"
    );
    assert_rejected(&input, &extra_base, "extra unsuffixed base job");
    Ok(())
}

#[test]
fn schema2_bun_renderer_rejects_noncanonical_paired_fields() -> Result<(), Box<dyn Error>> {
    let input = both_render_input()?;
    assert_renders(&input, &input.ir, "canonical paired fixture")?;
    assert_wrong_paired_selector(&input)?;

    let mut wrong_report_condition = input.ir.clone();
    let report = wrong_report_condition
        .jobs
        .get_mut(HOSTED_REPORT)
        .ok_or("paired fixture is missing its hosted browser report")?;
    let hosted_result = format!("needs.{HOSTED_BROWSER}.result");
    let local_result = format!("needs.{LOCAL_BROWSER}.result");
    let original_condition = report
        .condition
        .as_ref()
        .ok_or("hosted browser report has no condition")?
        .clone();
    let opposite_condition = original_condition.replace(&hosted_result, &local_result);
    assert_ne!(
        opposite_condition, original_condition,
        "hosted browser report condition does not name its own lane"
    );
    report.condition = Some(opposite_condition);
    assert_rejected(
        &input,
        &wrong_report_condition,
        "hosted report condition points at local browser suite",
    );

    let mut wrong_blob_pattern = input.ir.clone();
    let report = wrong_blob_pattern
        .jobs
        .get_mut(HOSTED_REPORT)
        .ok_or("paired fixture is missing its hosted browser report")?;
    replace_blob_pattern(report)?;
    assert_rejected(
        &input,
        &wrong_blob_pattern,
        "hosted report downloads local browser blob pattern",
    );
    Ok(())
}

fn assert_wrong_paired_selector(input: &BunRenderInput) -> Result<(), Box<dyn Error>> {
    let alternate = ScaleSetSelector::try_new(
        velnor_actions_contract::SCALE_SET_NAME,
        &[
            "velnor".to_owned(),
            velnor_actions_contract::SCALE_SET_NAME.to_owned(),
            "alternate-selector".to_owned(),
        ],
    )?;
    let mut wrong_selector = input.ir.clone();
    let original_selector = input
        .context
        .scale_set_selector
        .as_ref()
        .ok_or("paired fixture has no scale-set selector")?
        .token();
    let alternate_token = alternate.token();
    for logical_id in [
        "bun-fixture-ui",
        "bun-fixture-web",
        "bun-fixture-web--unit",
        "bun-fixture-web--browser",
        "bun-fixture-web--browser--report",
    ] {
        let local_id = format!("{logical_id}__local");
        let job = wrong_selector
            .jobs
            .get_mut(&local_id)
            .ok_or_else(|| format!("paired fixture is missing {local_id}"))?;
        assert_eq!(
            job.runs_on, original_selector,
            "{local_id} has an unexpected configured runner"
        );
        job.runs_on.clone_from(&alternate_token);
    }
    let mut wrong_selector_context = input.context.clone();
    wrong_selector_context.scale_set_selector = Some(alternate);
    assert_rejected_with_context(
        input,
        &wrong_selector,
        &wrong_selector_context,
        "wrong paired scale-set selector",
    );
    Ok(())
}

fn both_render_input() -> Result<BunRenderInput, Box<dyn Error>> {
    let repo = make_bun_repo()?;
    let migrated = migrate_config(repo.path(), 2, true)?;
    assert_eq!(
        fs::read_to_string(repo.path().join(".velnor/config.toml"))?,
        migrated
    );
    let prepared = prepare(repo.path())?;
    let selector = prepared
        .config
        .execution
        .as_ref()
        .ok_or("migrated config has no execution settings")?
        .scale_selector()?;
    let expansion = expand_workflow(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
    )?;
    let mut context = prepared.workflow.context.clone();
    context.bun_lane_plan = expansion.bun_lane_plan;
    context.scale_set_selector = Some(selector);
    Ok(BunRenderInput {
        ir: expansion.ir,
        policy: prepared.config.workflow.policy,
        support: prepared.workflow.support,
        context,
    })
}

fn fixture_mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA,
        ),
        version: velnor_actions_mise::MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}

fn assert_renders(
    input: &BunRenderInput,
    ir: &WorkflowIr,
    case: &str,
) -> Result<(), Box<dyn Error>> {
    let rendered = render_workflow_ir_strict(
        ir,
        input.policy,
        input.support.as_ref(),
        &input.context,
        &fixture_mise_setup(),
    )?;
    assert!(!rendered.is_empty(), "{case}: renderer produced empty YAML");
    Ok(())
}

fn assert_rejected(input: &BunRenderInput, ir: &WorkflowIr, case: &str) {
    assert_rejected_with_context(input, ir, &input.context, case);
}

fn assert_rejected_with_context(
    input: &BunRenderInput,
    ir: &WorkflowIr,
    context: &RenderContext,
    case: &str,
) {
    let rendered = render_workflow_ir_strict(
        ir,
        input.policy,
        input.support.as_ref(),
        context,
        &fixture_mise_setup(),
    );
    assert!(
        rendered.is_err(),
        "{case}: malformed expanded IR rendered successfully"
    );
}

fn replace_blob_pattern(report: &mut Job) -> Result<(), Box<dyn Error>> {
    let hosted_pattern = format!("velnor-pw-blob-{HOSTED_BROWSER}-leg-*");
    let local_pattern = format!("velnor-pw-blob-{LOCAL_BROWSER}-leg-*");
    let step_index = report
        .steps
        .iter()
        .position(|step| match &step.kind {
            StepKind::Action { uses, with, .. } => {
                uses.starts_with("actions/download-artifact@")
                    && with.get("pattern") == Some(&hosted_pattern)
            }
            _ => false,
        })
        .ok_or("hosted browser report has no hosted blob-pattern download")?;
    let StepKind::Action { with, .. } = &mut report.steps[step_index].kind else {
        return Err("located browser blob download is not an action".into());
    };
    *with
        .get_mut("pattern")
        .ok_or("hosted browser blob download lost its pattern")? = local_pattern;
    Ok(())
}
