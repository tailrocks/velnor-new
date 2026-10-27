// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use super::bun_routing::make_bun_repo;
use velnor_actions_contract::workflow::ir::WorkflowIr;
use velnor_actions_contract::{
    ExecutionMode, Job, ScaleSetSelector, StepKind, VelnorSupportWorkflow, WorkflowPolicy,
    expand_workflow_with_bun_plan,
};
use velnor_actions_orchestrator::{migrate_config, prepare};
use velnor_actions_workflow_renderer::{
    MiseSetup,
    render::{RenderContext, render_workflow_ir_strict},
    setup::MISE_BINARY_SHA256_LINUX_X64,
};

const BROWSER: &str = "bun-fixture-web--browser";
const REPORT: &str = "bun-fixture-web--browser--report";
const BUN_JOBS: [&str; 5] = [
    "bun-fixture-ui",
    "bun-fixture-web",
    BROWSER,
    REPORT,
    "bun-fixture-web--unit",
];

struct BunRenderInput {
    ir: WorkflowIr,
    policy: WorkflowPolicy,
    support: Option<VelnorSupportWorkflow>,
    context: RenderContext,
}

#[test]
fn schema2_bun_renderer_rejects_missing_and_extra_outputs() -> Result<(), Box<dyn Error>> {
    let input = render_input(ExecutionMode::Hosted)?;
    assert_renders(&input, &input.ir, "canonical fixture")?;

    let mut missing = input.ir.clone();
    assert!(missing.jobs.remove("bun-fixture-ui").is_some());
    missing
        .jobs
        .get_mut("required")
        .ok_or("fixture has no required job")?
        .needs
        .retain(|id| id != "bun-fixture-ui");
    assert_rejected(&input, &missing, "missing required Bun job");

    let mut extra = input.ir.clone();
    let member = extra
        .jobs
        .get("bun-fixture-ui")
        .ok_or("fixture has no UI Bun job")?
        .clone();
    assert!(
        extra
            .jobs
            .insert("bun-fixture-ui__illegal".to_owned(), member)
            .is_none()
    );
    assert_rejected(&input, &extra, "extra suffixed Bun job");
    Ok(())
}

#[test]
fn schema2_bun_renderer_rejects_noncanonical_fields() -> Result<(), Box<dyn Error>> {
    let input = render_input(ExecutionMode::ScaleSet)?;
    assert_renders(&input, &input.ir, "canonical scale-set fixture")?;

    let alternate = ScaleSetSelector::try_new(
        velnor_actions_contract::SCALE_SET_NAME,
        &[
            "velnor".to_owned(),
            velnor_actions_contract::SCALE_SET_NAME.to_owned(),
            "alternate-selector".to_owned(),
        ],
    )?;
    let original = input
        .context
        .scale_set_selector
        .as_ref()
        .ok_or("scale-set fixture has no selector")?
        .token();
    let mut wrong_selector = input.ir.clone();
    for id in BUN_JOBS {
        let job = wrong_selector
            .jobs
            .get_mut(id)
            .ok_or_else(|| format!("fixture has no {id}"))?;
        assert_eq!(job.runs_on, original);
        job.runs_on.clone_from(&alternate.token());
    }
    let mut wrong_context = input.context.clone();
    wrong_context.scale_set_selector = Some(alternate);
    assert_rejected_with_context(
        &input,
        &wrong_selector,
        &wrong_context,
        "wrong scale-set selector",
    );

    let mut wrong_condition = input.ir.clone();
    let report = wrong_condition
        .jobs
        .get_mut(REPORT)
        .ok_or("fixture has no browser report")?;
    let condition = report
        .condition
        .as_ref()
        .ok_or("browser report has no condition")?;
    let changed = condition.replace(
        &format!("needs.{BROWSER}.result"),
        "needs.bun-fixture-web--unit.result",
    );
    assert_ne!(changed, *condition);
    report.condition = Some(changed);
    assert_rejected(
        &input,
        &wrong_condition,
        "report condition names another suite",
    );

    let mut wrong_pattern = input.ir.clone();
    replace_blob_pattern(
        wrong_pattern
            .jobs
            .get_mut(REPORT)
            .ok_or("fixture has no browser report")?,
    )?;
    assert_rejected(
        &input,
        &wrong_pattern,
        "report downloads illegal suffixed blob pattern",
    );
    Ok(())
}

fn render_input(mode: ExecutionMode) -> Result<BunRenderInput, Box<dyn Error>> {
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
    let expansion =
        expand_workflow_with_bun_plan(&prepared.workflow.ir, &prepared.config, Some(mode))?;
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
            velnor_actions_actionlint::actions::MISE_ACTION_SHA
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
    let valid = format!("velnor-pw-blob-{BROWSER}-leg-*");
    let illegal = format!("velnor-pw-blob-{BROWSER}__illegal-leg-*");
    let index = report
        .steps
        .iter()
        .position(|step| match &step.kind {
            StepKind::Action { uses, with, .. } => {
                uses.starts_with("actions/download-artifact@")
                    && with.get("pattern") == Some(&valid)
            }
            _ => false,
        })
        .ok_or("browser report has no expected blob download")?;
    let StepKind::Action { with, .. } = &mut report.steps[index].kind else {
        return Err("blob download is not an action".into());
    };
    *with
        .get_mut("pattern")
        .ok_or("blob download lost pattern")? = illegal;
    Ok(())
}
