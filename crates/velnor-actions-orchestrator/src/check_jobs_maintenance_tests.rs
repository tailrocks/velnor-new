//! Repository-maintenance suites stay required without entering V1 products.

#[test]
fn canonical_maintenance_check_is_required_without_adding_product_tasks()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let preparation = crate::prepare(&root)?;
    let check = preparation
        .config
        .checks
        .iter()
        .find(|check| check.id == "maintenance-helpers")
        .ok_or("missing fixed repository-maintenance check")?;

    assert_eq!(check.task, "test-maintenance-helpers");
    assert_eq!(check.tools, ["cargo-nextest", "rust"]);
    assert_eq!(check.runner.label, "ubuntu-26.04");
    assert!(check.inputs.iter().any(|input| input == "mise.toml"));
    assert!(check.inputs.iter().any(|input| input == "Cargo.lock"));
    assert!(
        preparation
            .workflow
            .ir
            .jobs
            .contains_key("check-maintenance-helpers"),
        "the existing named-check contract emits the helper-suite job"
    );
    assert!(
        preparation.workflow.ir.jobs["required"]
            .needs
            .iter()
            .any(|id| id == "check-maintenance-helpers"),
        "Required gates the repository-maintenance suite"
    );
    assert!(
        preparation
            .discovery
            .proposals
            .iter()
            .any(|task| { task.task_id == "stack/mise/maintenance-helpers/check/default" }),
        "the check remains a named repository-quality obligation"
    );
    for task in &preparation.discovery.proposals {
        assert!(!task.task_id.contains("velnor-actions-freshness"));
        assert!(!task.task_id.contains("velnor-archive-guard"));
    }

    let mise = std::fs::read_to_string(root.join("mise.toml"))?;
    for package in ["velnor-actions-freshness", "velnor-archive-guard"] {
        assert!(
            mise.contains(&format!(
                "nextest run --locked --manifest-path Cargo.toml --package {package} --no-tests fail"
            )),
            "named check must discover and run tests for {package}"
        );
    }
    Ok(())
}
