use super::*;

/// A structurally valid local fixture; the digest and commit are synthetic.
fn schema_only_consumer_manifest() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = velnor_actions_contract::SUPPORTED_TARGETS
        .iter()
        .map(|target| {
            format!(
                "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "b".repeat(40)
    )
}

#[test]
fn consumer_crate_job_keeps_change_gate_with_or_without_seed_barrier() {
    let root = tempfile::tempdir().expect("repository");
    std::fs::create_dir(root.path().join(".velnor")).expect("config");
    let proposals = [TaskKind::Build, TaskKind::Clippy, TaskKind::Nextest]
        .into_iter()
        .map(|kind| group("demo", kind, &[]))
        .collect::<Vec<_>>();
    let mut found = discovery(proposals.clone());
    found.consumer_manifest_json = Some(schema_only_consumer_manifest());
    found.consumer_manifest_stand_in = true;

    for enabled in [false, true] {
        std::fs::write(
            root.path().join(".velnor/config.toml"),
            format!(
                "schema = 1\n[workflow]\npolicy = 'consumer-v1'\n[stacks.rust]\nseed_parallel_targets = {enabled}\n"
            ),
        )
        .expect("consumer config");
        let config = crate::config::load_config(root.path()).expect("load consumer config");
        let workflow = crate::workflow::build_workflow(
            root.path(),
            &config,
            "main",
            "ubuntu-26.04",
            &found,
            &[],
        )
        .expect("consumer workflow");

        let job = &workflow.ir.jobs["rust-demo"];
        let expected_gate = velnor_actions_workflow_renderer::not_affected_gate("rust-demo");
        assert_eq!(job.condition.as_deref(), Some(expected_gate.as_str()));
        assert!(
            workflow.ir.jobs["required"]
                .needs
                .iter()
                .any(|need| need == "rust-demo")
        );
        assert_seed_task_dispositions(job);
    }
}
