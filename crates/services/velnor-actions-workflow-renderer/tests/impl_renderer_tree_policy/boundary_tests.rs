use super::*;

#[test]
fn renderer_rejects_bare_commands_inside_ir() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(Step {
            name: "Install".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["cargo".to_owned(), "install".to_owned(), "x".to_owned()],
                env: BTreeMap::new(),
            },
        }),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(Step {
            name: "Fetch".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Action {
                uses: "actions/checkout@main".to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        }),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(Step {
            name: "Run Alint".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Action {
                uses: "asamarts/alint@v0.16.1".to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        }),
    );
    let err = render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx)
        .expect_err("alint tag ref must be rejected");
    assert!(
        format!("{err:?}").contains("unpinned_ref"),
        "wrong rejection: {err:?}"
    );
    let mut ir = fixture_ir()?;
    ir.jobs.insert(
        "velnor-task".to_owned(),
        task_job(velnor_actions_workflow_steps::shell_step(
            "Focused",
            vec!["true".to_owned()],
            BTreeMap::new(),
        )?),
    );
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_ok());
    Ok(())
}
