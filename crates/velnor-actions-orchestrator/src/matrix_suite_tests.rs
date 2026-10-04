//! Exact suite registry and pre-identity tool selection contracts.

use super::super::selected_crate_tools;
use super::*;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

#[test]
fn repository_suite_axes_have_one_exact_registry() {
    for (package, validators, tofu, python) in [
        ("velnor-actions-orchestrator", true, false, false),
        ("velnor-actions-cli", true, false, false),
        ("velnor-actions-mise", false, true, false),
        ("velnor-actions-native", false, false, true),
        ("velnor-actions-contract", false, false, false),
        ("velnor-actions-rust", false, false, false),
        ("velnor-actions-tofu", false, false, false),
        ("velnor-actions-workflow-renderer", false, false, false),
        ("velnor-actions-actionlint", false, false, false),
    ] {
        assert_eq!(
            crate_suite_tools(WorkflowPolicy::VelnorRepositoryV1, Some(package))
                .expect("audited suite"),
            SuiteTools {
                generate_validators: validators,
                opentofu: tofu,
                python,
            },
            "{package}",
        );
    }
}

#[test]
fn consumer_policy_retains_only_existing_suite_tools() {
    for package in [None, Some("velnor-actions-native"), Some("consumer-demo")] {
        let suite = crate_suite_tools(WorkflowPolicy::ConsumerV1, package).expect("consumer suite");
        assert_eq!(
            selected_crate_tools(&ToolCatalog::pinned(), false, false, false, false, suite),
            [
                PinnedTool::Actionlint,
                PinnedTool::Shellcheck,
                PinnedTool::Zizmor
            ],
        );
    }
}

#[test]
fn unknown_compiled_suites_fail_and_non_rust_work_has_no_suite() {
    assert!(crate_suite_tools(WorkflowPolicy::VelnorRepositoryV1, Some("unknown")).is_err());
    assert_eq!(
        crate_suite_tools(WorkflowPolicy::VelnorRepositoryV1, None).expect("no compiled suite"),
        SuiteTools::NONE,
    );
}

#[test]
fn native_python_enters_the_exact_tool_set_before_preparation() {
    let catalog = ToolCatalog::pinned();
    let suite = crate_suite_tools(
        WorkflowPolicy::VelnorRepositoryV1,
        Some("velnor-actions-native"),
    )
    .expect("native suite");
    assert_eq!(
        selected_crate_tools(&catalog, true, false, true, false, suite),
        [
            catalog.compiler_tool(),
            PinnedTool::Nextest,
            PinnedTool::Python
        ],
    );
    assert_eq!(
        selected_crate_tools(&catalog, false, false, false, false, suite),
        [PinnedTool::Python],
        "Python selection never implies Rust",
    );
}

#[test]
fn driver_and_suite_opentofu_share_one_install_slot() {
    let catalog = ToolCatalog::pinned();
    let suite = crate_suite_tools(
        WorkflowPolicy::VelnorRepositoryV1,
        Some("velnor-actions-mise"),
    )
    .expect("Mise suite");
    assert_eq!(
        selected_crate_tools(&catalog, false, false, false, true, suite),
        [PinnedTool::Opentofu],
    );
}
