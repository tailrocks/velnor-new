use super::*;

#[test]
fn ignored_rust_candidate_keeps_plan_inventory_toolchain() {
    use velnor_actions_contract::{DetectedProject, DetectionStatus};
    let (root, mut discovery) = fixture(&[]);
    discovery.statuses.push(DetectionStatus::Ignored {
        project: DetectedProject {
            stack_id: "rust".to_owned(),
            project_root: String::new(),
            manifest: "Cargo.toml".to_owned(),
        },
        reason: "configured_ignore".to_owned(),
    });
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\npolicy = 'velnor-repository-v1'\n",
    )
    .expect("config");
    let config = crate::config::load_config(root.path()).expect("defaulted config");
    let workflow = named_workflow(root.path(), &config, &discovery);
    let plan = &workflow.ir.jobs["plan"];
    let pinned_tools = plan
        .steps
        .iter()
        .find(|step| step.name == "Prepare pinned tools")
        .expect("plan inventory prepares its exact Rust compiler");
    assert!(matches!(
        &pinned_tools.kind,
        StepKind::Shell { run, .. }
            if run.contains(&ToolCatalog::pinned().tool_spec(PinnedTool::Rust))
    ));
    assert!(
        !plan
            .steps
            .iter()
            .any(|step| step.name == "Prepare Rust components"),
        "an ignored candidate has no selected plan-owned Format step"
    );
}
