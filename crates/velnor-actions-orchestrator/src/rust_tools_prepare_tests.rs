//! Source-owner admission and preparation-domain regressions.

use super::{prepare_step, prepare_step_in_domain};
use std::collections::BTreeMap;
use velnor_actions_contract::{HelperInvocation, Step, StepKind};
use velnor_actions_mise::catalog::rust_prepare::{self, RustPrepareDomain};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

fn helper(step: &Step) -> (&HelperInvocation, &BTreeMap<String, String>) {
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        panic!("Rust preparation must bind source-owner authority");
    };
    (invocation, env)
}

fn install(catalog: &ToolCatalog, tools: &[PinnedTool]) -> Vec<String> {
    std::iter::once("mise".to_owned())
        .chain(
            velnor_actions_mise::MISE_GLOBAL_FLAGS
                .iter()
                .map(|flag| (*flag).to_owned()),
        )
        .chain(std::iter::once("install".to_owned()))
        .chain(
            tools
                .iter()
                .map(|tool| catalog.tool_spec(*tool).expect("qualified selector")),
        )
        .collect()
}

#[test]
fn plan_and_matrix_share_compiled_source_owner() {
    let catalog = ToolCatalog::pinned();
    let plan = crate::workflow_jobs::prepare_pinned_tools_step_for_runner(
        &catalog,
        vec![
            PinnedTool::Rust,
            PinnedTool::MrBoxington,
            PinnedTool::Nextest,
        ],
        true,
        "ubuntu-24.04",
    )
    .expect("plan preparation");
    let matrix = crate::matrix_step::prepare_crate_tools_step(
        &catalog,
        true,
        true,
        true,
        false,
        crate::matrix_step::SuiteTools::NONE,
        "ubuntu-24.04",
    )
    .expect("matrix preparation");
    assert_eq!(helper(&plan), helper(&matrix));
    let (invocation, env) = helper(&plan);
    let record = rust_prepare::record_for_invocation(invocation, env, env!("CARGO_PKG_VERSION"))
        .expect("owner admission");
    assert_eq!(record.invocation(), invocation);
    assert_eq!(env["CARGO_HOME"], env["MISE_CARGO_HOME"]);
    assert_eq!(env["RUSTUP_HOME"], env["MISE_RUSTUP_HOME"]);
    assert_eq!(env["RUSTUP_TOOLCHAIN"], catalog.rust_toolchain_name());
    assert!(!env.contains_key("BASH_ENV"), "launcher owns startup scrub");
}

#[test]
fn domain_changes_argument_authority_with_same_compiled_source() {
    let catalog = ToolCatalog::pinned();
    let argv = install(&catalog, &[PinnedTool::Rust]);
    let tools = prepare_step(argv.clone(), BTreeMap::new(), true, &catalog).expect("tools");
    let planning = prepare_step_in_domain(
        argv.clone(),
        BTreeMap::new(),
        true,
        &catalog,
        RustPrepareDomain::PlanningBootstrap,
    )
    .expect("planning");
    let (tools_invocation, _) = helper(&tools);
    let (planning_invocation, _) = helper(&planning);
    assert_eq!(
        tools_invocation.descriptor(),
        planning_invocation.descriptor()
    );
    assert_eq!(tools_invocation.args()[0], "tools");
    assert_eq!(planning_invocation.args()[0], "planning-bootstrap");
    assert_eq!(
        rust_prepare::install_argv(planning_invocation, env!("CARGO_PKG_VERSION")),
        Some(argv)
    );
}

#[test]
fn owner_rejects_changed_environment_and_invocation() {
    let catalog = ToolCatalog::pinned();
    let step = prepare_step(
        install(&catalog, &[PinnedTool::Rust]),
        BTreeMap::new(),
        true,
        &catalog,
    )
    .expect("owner step");
    let (invocation, env) = helper(&step);
    let mut changed = env.clone();
    changed.insert("CARGO_HOME".to_owned(), "/tmp/foreign".to_owned());
    assert!(
        rust_prepare::record_for_invocation(invocation, &changed, env!("CARGO_PKG_VERSION"))
            .is_err()
    );
    let mut encoded = serde_json::to_value(invocation).expect("serialize");
    encoded["args"][0] = serde_json::json!("foreign-domain");
    let changed: HelperInvocation = serde_json::from_value(encoded).expect("deserialize");
    assert!(rust_prepare::install_argv(&changed, env!("CARGO_PKG_VERSION")).is_none());
}

#[test]
fn non_rust_preparation_keeps_plain_install_and_toolset_identity() {
    let catalog = ToolCatalog::pinned();
    let argv = install(&catalog, &[PinnedTool::Gh]);
    let step = prepare_step(argv.clone(), BTreeMap::new(), false, &catalog).expect("non Rust");
    let StepKind::Shell { run, env } = &step.kind else {
        panic!("plain Mise install");
    };
    assert_eq!(run, &argv);
    assert!(env["VELNOR_TOOL_CACHE_IDENTITY"].starts_with("toolset@"));
    for key in [
        "RUSTUP_HOME",
        "CARGO_HOME",
        "RUSTUP_VERSION",
        "VELNOR_RUSTUP_IDENTITY",
    ] {
        assert!(!env.contains_key(key));
    }
}

#[test]
fn desktop_owner_binds_desktop_compiler_and_driver() {
    let desktop = ToolCatalog::pinned()
        .for_native_kind("native_xcode_project_ci")
        .expect("desktop");
    let argv = install(
        &desktop,
        &[PinnedTool::RustDesktop, PinnedTool::MrBoxington],
    );
    let step = prepare_step(argv.clone(), BTreeMap::new(), true, &desktop).expect("desktop owner");
    let (invocation, env) = helper(&step);
    assert_eq!(
        rust_prepare::install_argv(invocation, env!("CARGO_PKG_VERSION")),
        Some(argv)
    );
    assert_eq!(env["RUSTUP_TOOLCHAIN"], desktop.rust_toolchain_name());
    assert_eq!(
        invocation.descriptor().path(),
        ".github/velnor/rust_prepare_desktop_mac.sh"
    );
}
