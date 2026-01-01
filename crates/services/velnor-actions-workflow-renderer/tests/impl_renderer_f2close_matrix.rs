//! F2 closure: matrix reports, write-request insertion, plan outputs.
use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_workflow_renderer::{PLAN_ID_OUTPUT, RUN_KEY_OUTPUT, render_workflow_ir};
use velnor_actions_workflow_steps::{
    MATRIX_REPORT_UPLOAD_NAME, RenderError, checkout_step, merge_step, plan_step,
    write_request_step,
};

use super::impl_renderer_fixtures::*;
#[test]
fn task_job_gains_matrix_report_upload() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, matrix_task_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let start = text.find("velnor-task:").expect("task job");
    let end = text[start..]
        .find("plan:")
        .map_or(text.len(), |at| start + at);
    let window = &text[start..end.min(text.len())];
    let full = if end <= start { &text[start..] } else { window };
    assert!(full.contains(MATRIX_REPORT_UPLOAD_NAME), "upload:\n{full}");
    assert!(
        full.contains("velnor-matrix-r${{ github.run_id }}-a${{ github.run_attempt }}-${{ matrix.matrix_key }}"),
        "derived name:\n{full}"
    );
    let at = full.find(MATRIX_REPORT_UPLOAD_NAME).expect("upload step");
    assert!(
        snip(full, at, 500).contains("if: always()"),
        "if always:\n{full}"
    );
    assert_eq!(full.matches(MATRIX_REPORT_UPLOAD_NAME).count(), 1);
    Ok(())
}

#[test]
fn matrix_upload_absent_without_task_job() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        !text.contains(MATRIX_REPORT_UPLOAD_NAME),
        "phantom:\n{text}"
    );
    Ok(())
}

#[test]
fn write_request_inserted_before_plan_and_merge() -> Result<(), RenderError> {
    let mut final_job = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![acquire_fixture()?, merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let text = render_workflow_ir(
        &fixture_ir(vec![
            minimal_plan_job()?,
            ("required".to_owned(), final_job),
        ]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    for (consumer, target) in [("Plan", "plan-v1"), ("Merge reports", "merge-v1")] {
        let write = text.find("Write request").expect("write step");
        let use_at = text[write..]
            .find(consumer)
            .unwrap_or_else(|| panic!("{consumer} after write:\n{text}"));
        assert!(use_at > 0, "{consumer} order:\n{text}");
        assert!(
            text.contains(&format!("write-request-v1:{target}"))
                || text.contains("write-request-v1"),
            "op {target}:\n{text}"
        );
    }
    assert_eq!(text.matches("Write request").count(), 2, "both:\n{text}");
    Ok(())
}

#[test]
fn write_request_insertion_idempotent() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            write_request_step("plan-v1")?,
            plan_step(),
        ],
    );
    let text = render_workflow_ir(
        &fixture_ir(vec![plan]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert_eq!(text.matches("Write request").count(), 1, "dup:\n{text}");
    Ok(())
}

#[test]
fn plan_outputs_publish_plan_id_run_key_and_matrix() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, matrix_task_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    for line in [
        "matrix: ${{ steps.plan.outputs.matrix }}",
        "plan_id: ${{ steps.plan.outputs.plan_id }}",
        "run_key: ${{ steps.plan.outputs.run_key }}",
    ] {
        assert!(text.contains(line), "missing {line}:\n{text}");
    }
    assert_eq!(PLAN_ID_OUTPUT, "plan_id");
    assert_eq!(RUN_KEY_OUTPUT, "run_key");
    Ok(())
}
