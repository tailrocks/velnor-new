//! Real preparation and obligation planning bind the emitted runner.

use velnor_actions_contract::WorkflowEvent;
use velnor_actions_mise::ToolCatalog;

use crate::internal::plan_obligation::{GroupInputs, plan_group};
use crate::internal_plan::identities::platform_id_for_group;
use crate::internal_plan::snapshot::{ExecutionSnapshot, platform_id_for, platform_inputs_for};
use crate::internal_plan::wire_w2::GroupWire;

#[test]
fn swift_planned_platform_matches_generated_macos_job() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::TempDir::new()?;
    std::fs::create_dir_all(root.path().join(".velnor"))?;
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\ndefault_branch = \"main\"\n[[stacks.workloads]]\nname = \"swift\"\nkind = \"swift_test\"\n",
    )?;
    std::fs::write(root.path().join("Package.swift"), "// swift fixture\n")?;
    let prep = crate::prepare(root.path())?;
    let job = prep
        .workflow
        .ir
        .jobs
        .values()
        .find(|job| job.runs_on == "macos-26")
        .ok_or("missing macOS job")?;
    let platform = platform_inputs_for(&job.runs_on, "aarch64-apple-darwin")?;
    assert_eq!(platform.runs_on, "macos-26");
    assert_eq!(platform.arch, "aarch64");
    assert_eq!(platform.os, "macos");
    let expected = platform_id_for(&job.runs_on, "aarch64-apple-darwin")?;
    let wrong = platform_id_for("ubuntu-26.04", "aarch64-apple-darwin")?;
    assert_ne!(expected, wrong);
    let snapshot = ExecutionSnapshot::build(&prep.discovery).with_checkout(root.path());
    let generator = crate::internal_plan::default_generator();
    let catalog = ToolCatalog::pinned();
    for task in &prep.discovery.proposals {
        assert_eq!(platform_id_for_group("ubuntu-26.04", task)?, expected);
        let (_, entry) = plan_group(
            &GroupInputs {
                discovery: &prep.discovery,
                task,
                run_key: "local",
                label: "ubuntu-26.04",
                catalog: &catalog,
                wire: GroupWire {
                    event: WorkflowEvent::Local,
                    generator: &generator,
                },
                changed: true,
                snapshot: &snapshot,
                root: root.path(),
            },
            &mut velnor_actions_tofu::FileCache::new(),
        )?;
        assert_eq!(
            entry
                .cache_ids
                .ok_or("missing cache identities")?
                .platform_id(),
            expected
        );
    }
    let rendered = crate::render_staged_tree(&prep)?;
    let yaml = rendered
        .get(".github/workflows/ci.yml")
        .ok_or("missing CI")?;
    assert!(yaml.contains("macos-26"));
    Ok(())
}
