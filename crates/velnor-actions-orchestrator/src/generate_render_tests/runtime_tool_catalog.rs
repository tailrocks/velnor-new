use std::fs;
use std::path::Path;

use velnor_actions_contract::{ExecutionMode, StepKind, StepRole};
use velnor_actions_mise::PinnedTool;
use velnor_actions_workflow_renderer::{PRESEED_MANIFEST_TOOLCHAIN_ENV, render::PLAN_JOB_ID};

#[test]
fn scale_set_preseed_uses_project_rust_pin_without_workflow_regeneration() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = super::isolated_generation_root(&source);
    fs::remove_file(root.path().join(".velnor/generator.lock"))
        .expect("fixture uses the native pre-seed path without a release lock");

    let prep = crate::prepare(root.path()).expect("project toolchain sources resolve");
    let first = preseed_ci_for_rust_pin(&prep, root.path(), "1.99.1");
    set_project_rust_pin(root.path(), "1.99.1", "1.99.0");
    let second = preseed_ci_for_rust_pin(&prep, root.path(), "1.99.0");

    assert_eq!(
        first, second,
        "a project Rust pin bump must not change emitted workflow bytes"
    );
}

fn preseed_ci_for_rust_pin(
    source: &crate::prepare::GenerationPreparation,
    root: &Path,
    expected_rust: &str,
) -> String {
    let mut prep = source.clone();
    prep.catalog = crate::tool_catalog_resolve::resolve_tool_catalog(root, &prep.config)
        .expect("project Rust source resolves without tool availability checks");
    assert_eq!(prep.catalog.version(PinnedTool::Rust), expected_rust);
    let owned = crate::finalized::owned_preparation(&prep)
        .expect("the lockless Velnor repository uses native pre-seed");
    assert!(owned.workflow.context.preseed);

    let plan = &owned.workflow.ir.jobs[PLAN_JOB_ID];
    let step = |role| {
        plan.steps
            .iter()
            .find(|step| step.role == Some(role))
            .unwrap_or_else(|| panic!("missing pre-seed step {role:?}"))
    };
    let build = step(StepRole::PreseedBuild);
    let StepKind::Shell { run, .. } = &build.kind else {
        panic!("pre-seed build is a shell command");
    };
    let command = run.join(" ");
    assert!(
        command.contains("'rust@'\"${VELNOR_TOOL_RUST}\""),
        "pre-seed build must use the resolved project Rust version: {command}"
    );
    assert!(!command.contains(expected_rust), "{command}");

    let verify = step(StepRole::PreseedVerifyBuild);
    let StepKind::Shell { run, .. } = &verify.kind else {
        panic!("MBX verification is a shell command");
    };
    let command = run.join(" ");
    assert!(
        command.contains("'rust@'\"${VELNOR_TOOL_RUST}\""),
        "MBX verification must use the resolved project Rust version: {command}"
    );
    assert!(!command.contains(expected_rust), "{command}");

    let mbx = step(StepRole::MbxCache);
    let StepKind::Action { with, .. } = &mbx.kind else {
        panic!("MBX provisioning is an action");
    };
    assert_eq!(
        with.get("toolchain").map(String::as_str),
        Some("${VELNOR_TOOL_RUST}")
    );

    let manifest = step(StepRole::PreseedManifest);
    let StepKind::Shell { env, .. } = &manifest.kind else {
        panic!("pre-seed manifest writer is a shell command");
    };
    assert_eq!(
        env.get(PRESEED_MANIFEST_TOOLCHAIN_ENV).map(String::as_str),
        Some("rust@${VELNOR_TOOL_RUST}")
    );

    let tree = super::super::render_all(&owned, Some(ExecutionMode::ScaleSet))
        .expect("Scale Set workflow renders from the pre-seed plan");
    tree.files
        .iter()
        .find(|file| file.path == ".github/workflows/ci.yml")
        .expect("CI workflow is generated")
        .bytes
        .clone()
}

fn set_project_rust_pin(root: &Path, from: &str, to: &str) {
    for relative in [
        "mise.toml",
        "rust-toolchain.toml",
        "fixtures/mbx-nextest/mise.toml",
    ] {
        let path = root.join(relative);
        let source = fs::read_to_string(&path).expect("Rust source file is readable");
        let updated = source.replace(from, to);
        assert_ne!(
            source, updated,
            "{relative} contains the old project Rust pin"
        );
        fs::write(path, updated).expect("fixture source pin is updated");
    }
}
