//! Prove that each maintenance package has one Rust job.

use std::error::Error;

#[test]
fn maintenance_packages_use_one_rust_job_each() -> Result<(), Box<dyn Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let preparation = crate::prepare(&root)?;
    assert!(
        !preparation
            .config
            .checks
            .iter()
            .any(|check| check.id == "maintenance-helpers"),
        "package tests must use their Rust jobs"
    );

    for package in ["velnor-actions-freshness", "velnor-archive-guard"] {
        let job_id = format!("rust-{package}");
        let matching_jobs = preparation
            .workflow
            .ir
            .jobs
            .keys()
            .filter(|id| id.starts_with(&job_id))
            .collect::<Vec<_>>();
        assert_eq!(matching_jobs, [&job_id]);
        assert!(
            preparation.workflow.ir.jobs[&job_id]
                .needs
                .contains(&"plan".to_owned()),
            "Required Rust job must use the plan"
        );
        for task in ["fmt", "clippy", "nextest", "doctest"] {
            let task_id = format!("stack/rust/crates/{package}/{task}/default");
            assert!(
                preparation
                    .discovery
                    .proposals
                    .iter()
                    .any(|proposal| proposal.task_id == task_id),
                "missing Rust task {task_id}"
            );
        }
    }

    let mise = std::fs::read_to_string(root.join("mise.toml"))?;
    assert!(mise.contains("[tasks.test-maintenance-helpers]"));
    for package in ["velnor-actions-freshness", "velnor-archive-guard"] {
        assert!(mise.contains(&format!(
            "nextest run --locked --manifest-path Cargo.toml --package {package} --no-tests fail"
        )));
    }
    Ok(())
}
