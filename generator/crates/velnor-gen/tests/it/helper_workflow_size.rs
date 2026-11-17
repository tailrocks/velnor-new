// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::helper_integrity::{steps, workflow_text};
use velnor_model::{CargoCommand, PackageName, StepKind, WORKFLOW_PATH};
use velnor_render::{RenderedJob, RenderedWorkflow, Rule, render, render_workflow};

#[test]
fn hundreds_of_helper_calls_share_one_workflow_owned_launcher() -> TestResult {
    let extra = (0..320)
        .map(|index| {
            Ok(StepKind::Cargo(CargoCommand::CheckAlone {
                package: PackageName::new(format!("package-{index}"))?,
            }))
        })
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    let text = workflow_text(&"a".repeat(64), extra)?;
    assert!(text.len() <= 500_000, "{} bytes", text.len());
    assert_eq!(text.matches("VELNOR_HELPER_COPY=$(").count(), 1);
    let yaml = crate::yaml_tree::document(&text)?;
    let steps = steps(&yaml)?;
    let invocations: Vec<_> = steps
        .iter()
        .filter(|step| step["env"]["VELNOR_HELPER_LAUNCHER"].is_string())
        .collect();
    assert_eq!(invocations.len(), 324);
    let launcher = &invocations[0]["env"]["VELNOR_HELPER_LAUNCHER"];
    for invocation in &invocations {
        assert_eq!(&invocation["env"]["VELNOR_HELPER_LAUNCHER"], launcher);
    }
    let gate = invocations
        .iter()
        .find(|step| step["name"] == "velnor-ci gate")
        .ok_or("no gate")?;
    assert_eq!(gate["if"], "${{ always() }}");
    Ok(())
}

#[test]
fn workflow_size_admission_counts_final_utf8_bytes_at_the_limit() -> TestResult {
    let plan = crate::rules::plan(Vec::new())?;
    let mut rendered = RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "format".to_owned(),
            name: Some("x".to_owned()),
            runs_on: "ubuntu-latest".to_owned(),
            timeout_minutes: 10,
            ..RenderedJob::default()
        }],
    };
    let initial = render_workflow(&plan, &rendered).map_err(|errors| format!("{errors:?}"))?;
    let baseline = initial.get(&WORKFLOW_PATH).ok_or("no workflow")?.len();
    rendered.jobs[0].name = Some("x".repeat(500_000 - baseline + 1));
    let accepted = render_workflow(&plan, &rendered).map_err(|errors| format!("{errors:?}"))?;
    assert_eq!(
        accepted.get(&WORKFLOW_PATH).ok_or("no workflow")?.len(),
        500_000
    );
    rendered.jobs[0].name.as_mut().ok_or("no name")?.push('x');
    let rejected = render_workflow(&plan, &rendered);
    assert!(rejected.is_err_and(|errors| {
        errors
            .iter()
            .any(|error| error.rule == Rule::WorkflowTooLarge(500_001))
    }));
    let unicode = "é".repeat(250_001);
    rendered.jobs[0].name = Some(unicode.clone());
    let rejected = render_workflow(&plan, &rendered);
    let expected = baseline - 1 + unicode.len() + 2;
    assert!(rejected.is_err_and(|errors| {
        errors
            .iter()
            .any(|error| error.rule == Rule::WorkflowTooLarge(expected))
    }));
    let mut model = plan;
    model.name = "x".repeat(500_001);
    assert!(render(&model).is_err_and(|errors| {
        errors
            .iter()
            .any(|error| matches!(error.rule, Rule::WorkflowTooLarge(_)))
    }));
    Ok(())
}
