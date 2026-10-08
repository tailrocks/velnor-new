use super::*;

#[test]
fn unsupported_dynamic_contexts_fail_closed() {
    for expression in [
        "inputs.untrusted == 'x'",
        "matrix.target == 'x'",
        "strategy.job-index > 0",
    ] {
        let mut steps = task_job_steps(&format!(
            "!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')"
        ));
        steps[1].name = "prepare tools".to_owned();
        steps[1].condition = Some(expression.to_owned());
        let jobs = BTreeMap::from([(JOB_ID.to_owned(), task_job(steps))]);
        let error = share_lanes(&jobs, &ctx()).expect_err("unsupported context rejected");
        assert!(
            matches!(
                error,
                RenderError::InvalidWorkflow(ref detail)
                    if detail == "task_composite_unsupported_context:prepare tools"
            ),
            "{expression}: {error:?}"
        );
    }
}

#[test]
fn bracketed_context_access_in_task_body_fails_closed() {
    for expression in [
        "${{ needs['other'].outputs.covered_tasks }}",
        "${{ inputs['covered_tasks'] }}",
        "${{ matrix['target'] }}",
        "${{ steps['prepare'].outputs.value }}",
    ] {
        let mut steps = task_job_steps(&format!(
            "!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')"
        ));
        steps[1].name = "prepare tools".to_owned();
        let StepKind::Shell { run, .. } = &mut steps[1].kind else {
            panic!("preparation is a shell step");
        };
        run.push(expression.to_owned());
        let jobs = BTreeMap::from([(JOB_ID.to_owned(), task_job(steps))]);
        let error = share_lanes(&jobs, &ctx()).expect_err("indexed context is unsupported");
        assert!(
            matches!(
                error,
                RenderError::InvalidWorkflow(ref detail)
                    if detail == "task_composite_unsupported_context:prepare tools"
            ),
            "{expression}: {error:?}"
        );
    }
}

#[test]
fn bracketed_step_output_in_postlude_fails_closed() {
    let mut steps = task_job_steps(&format!(
        "!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')"
    ));
    let StepKind::Action { env, .. } = &mut steps[6].kind else {
        panic!("report upload is an action");
    };
    env.insert(
        "REPORT_REF".to_owned(),
        "${{ steps['prepare'].outputs.value }}".to_owned(),
    );
    let jobs = BTreeMap::from([(JOB_ID.to_owned(), task_job(steps))]);
    let error = share_lanes(&jobs, &ctx()).expect_err("indexed postlude reference rejected");
    assert!(matches!(
        error,
        RenderError::InvalidWorkflow(ref detail)
            if detail == "task_composite_outer_step_order_unsupported:rust-demo"
    ));
}
