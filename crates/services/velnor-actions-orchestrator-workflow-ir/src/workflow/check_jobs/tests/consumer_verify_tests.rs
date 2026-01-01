use super::*;

fn consumer_fixture() -> (tempfile::TempDir, Discovery) {
    let (root, mut discovery) = fixture(&[]);
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    let version = env!("CARGO_PKG_VERSION");
    let targets = velnor_actions_contract_release::SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    discovery.consumer_manifest_json = Some(format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "a".repeat(40)
    ));
    (root, discovery)
}

fn consumer_plan(
    root: &tempfile::TempDir,
    discovery: &Discovery,
    config_source: &str,
) -> crate::workflow::WorkflowPlan {
    std::fs::write(root.path().join(".velnor/config.toml"), config_source)
        .expect("workflow config");
    let config = velnor_actions_orchestrator_core::config::load_config(root.path())
        .expect("valid ConsumerV1 config");
    crate::workflow::build_workflow(&config, "main", "ubuntu-26.04", discovery, &[])
        .expect("ConsumerV1 workflow plan")
}

fn render_consumer_plan(plan: &crate::workflow::WorkflowPlan) -> String {
    velnor_actions_workflow_renderer::render_workflow_ir(
        &plan.ir,
        WorkflowPolicy::ConsumerV1,
        plan.support.as_ref(),
        &plan.context,
    )
    .expect("ConsumerV1 workflow render")
}

#[test]
fn default_consumer_config_omits_zizmor() {
    let (root, discovery) = consumer_fixture();
    let plan = consumer_plan(&root, &discovery, "schema = 1\n");

    assert!(plan.support.is_none());
    assert!(plan.context.validator_commands.iter().all(|command| {
        command.validator != velnor_actions_contract_config::ValidatorKind::Zizmor
    }));
    assert!(!render_consumer_plan(&plan).contains("  zizmor:"));
}

#[test]
fn selected_zizmor_is_typed_and_required() {
    let (root, discovery) = consumer_fixture();
    let plan = consumer_plan(
        &root,
        &discovery,
        "schema = 1\n[workflow.verify]\njobs = [\"zizmor\"]\n",
    );
    let support = plan.support.as_ref().expect("selected support lane");
    assert_eq!(
        support.validators,
        [velnor_actions_contract_config::ValidatorKind::Zizmor]
    );
    let command = plan
        .context
        .validator_commands
        .iter()
        .find(|command| command.validator == velnor_actions_contract_config::ValidatorKind::Zizmor)
        .expect("typed Zizmor command");
    assert_eq!(command.name, "Run zizmor");
    assert!(command.argv.iter().any(|argument| argument == "zizmor"));

    let yaml = render_consumer_plan(&plan);
    assert!(yaml.contains("  zizmor:"), "{yaml}");
    let required = yaml.split("  required:").nth(1).expect("Required job");
    assert!(
        required.contains("- zizmor"),
        "Required must wait for Zizmor: {required}"
    );
}
