//! Full homes follow the data domain without selecting a compiler.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, ToolCacheDomain, WorkflowPolicy};
use velnor_actions_mise::{PREPARE_PINNED_TOOLS_STEP, PinnedTool, ToolCatalog};

use super::plan_job;

fn shell(step: &Step) -> (&[String], &BTreeMap<String, String>) {
    let StepKind::Shell { run, env } = &step.kind else {
        panic!("compiler-free preparation remains a shell installer");
    };
    (run, env)
}

fn assert_python_full(step: &Step) {
    let (run, env) = shell(step);
    for (key, value) in ToolCacheDomain::Full.home_environment() {
        assert_eq!(env.get(&key), Some(&value), "Full owns {key}");
    }
    assert!(!env.contains_key("RUSTUP_TOOLCHAIN"));
    let installed = &run[run
        .iter()
        .position(|arg| arg == "install")
        .expect("install")
        + 1..];
    assert_eq!(installed.len(), 1, "homes never add installations");
    let catalog = ToolCatalog::pinned();
    let host = crate::workloads::host_for_runner("macos-26").expect("native fixture host");
    let expected = catalog
        .native_tool_specs(host, &[PinnedTool::Python])
        .expect("Python selector");
    assert_eq!(installed, expected);
    assert!(
        run.iter()
            .all(|arg| !arg.contains("rust@") && !arg.contains("mr-boxington@"))
    );
}

#[test]
fn workflow_python_only_full_preparation_has_home_authority_without_compiler_install() {
    let step = super::prepare_pinned_tools_step_for_runner(
        &ToolCatalog::pinned(),
        vec![PinnedTool::Python],
        false,
        "macos-26",
    )
    .expect("qualified native Python preparation");
    assert_python_full(&step);
}

#[test]
fn matrix_python_only_full_preparation_has_the_same_home_authority() {
    let suite = crate::matrix_step::crate_suite_tools(
        WorkflowPolicy::VelnorRepositoryV1,
        Some("velnor-actions-native"),
    )
    .expect("actual native suite");
    let step = crate::matrix_step::prepare_crate_tools_step(
        &ToolCatalog::pinned(),
        false,
        false,
        false,
        false,
        suite,
        "macos-26",
    )
    .expect("qualified native Python preparation");
    assert_python_full(&step);
}

#[test]
fn planning_gh_preparation_retains_its_compiler_free_home_free_domain() {
    let step = super::prepare_planning_tools_step(&ToolCatalog::pinned(), vec![PinnedTool::Gh])
        .expect("planning Gh preparation");
    let (run, env) = shell(&step);
    for (key, _) in ToolCacheDomain::Full.home_environment() {
        assert!(!env.contains_key(&key), "Planning owns no {key}");
    }
    assert!(!env.contains_key("RUSTUP_TOOLCHAIN"));
    let installed = &run[run
        .iter()
        .position(|arg| arg == "install")
        .expect("install")
        + 1..];
    assert_eq!(installed.len(), 1);
    let expected = ToolCatalog::pinned()
        .tool_spec(PinnedTool::Gh)
        .expect("Gh selector");
    assert_eq!(installed, [expected]);
}

#[test]
fn pure_tofu_plan_drops_all_rust_setup() {
    use velnor_actions_mise::PREPARE_RUST_COMPONENTS_STEP;

    use crate::source_prep::FETCH_SOURCES_STEP;
    let catalog = ToolCatalog::pinned();
    let job = plan_job(
        "ubuntu-26.04",
        None,
        &catalog,
        false,
        false,
        false,
        true,
        &[],
    )
    .expect("plan job");
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    assert!(
        !names.contains(&PREPARE_RUST_COMPONENTS_STEP),
        "no components step: {names:?}"
    );
    assert!(
        !names
            .iter()
            .any(|name| name.starts_with(FETCH_SOURCES_STEP)),
        "no {FETCH_SOURCES_STEP}: {names:?}"
    );
    let prepare_at = names
        .iter()
        .position(|name| *name == PREPARE_PINNED_TOOLS_STEP)
        .expect("prepare step");
    let StepKind::Shell { run, env } = &job.steps[prepare_at].kind else {
        panic!("prepare must be a shell step: {names:?}");
    };
    let install_at = run
        .iter()
        .position(|arg| arg == "install")
        .expect("install argv");
    let specs = &run[install_at + 1..];
    assert_eq!(
        specs,
        [
            catalog
                .tool_spec(PinnedTool::Actionlint)
                .expect("qualified selector"),
            catalog
                .tool_spec(PinnedTool::Shellcheck)
                .expect("qualified selector"),
            catalog
                .tool_spec(PinnedTool::Zizmor)
                .expect("qualified selector"),
            catalog
                .native_tool_spec(
                    velnor_actions_mise::catalog::qualification::DistributionHost::LinuxAmd64,
                    PinnedTool::Opentofu,
                )
                .expect("qualified selector"),
        ]
        .as_slice(),
        "pure-tofu plan installs opentofu plus validators: {run:?}"
    );
    for (key, value) in ToolCacheDomain::Full.home_environment() {
        assert_eq!(env.get(&key), Some(&value), "Full owns {key}");
    }
    assert!(!env.contains_key("RUSTUP_TOOLCHAIN"));
    assert_eq!(
        env.get("MISE_NO_CONFIG").map(String::as_str),
        Some("1"),
        "isolation overlay stays: {env:?}"
    );
}
