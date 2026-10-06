//! Closed workload operations, tool requirements and phase presentation.

use super::{desktop, security};
use velnor_actions_contract::ContractError;
use velnor_actions_contract::config::{WorkloadConfig, WorkloadKind};
use velnor_actions_mise::PinnedTool;

pub(super) fn phases(
    workload: &WorkloadConfig,
) -> Result<Vec<(&'static str, Vec<String>)>, crate::OrchestratorError> {
    let command =
        |phase, args: &[&str]| (phase, args.iter().map(|arg| (*arg).to_owned()).collect());
    let phases = match workload.kind {
        WorkloadKind::DockerBuild => vec![(
            "build",
            vec![
                "docker".into(),
                "build".into(),
                "--file".into(),
                "Dockerfile".into(),
                "--tag".into(),
                "local-ci:validation".to_owned(),
                ".".into(),
            ],
        )],
        WorkloadKind::BunCi | WorkloadKind::NodeCi => velnor_actions_native::node::phases(workload),
        WorkloadKind::NativeXcodeProjectCi | WorkloadKind::NativeSwiftPackageCi => {
            return desktop::phases(workload, "$VELNOR_NATIVE_SOURCE_SHA");
        }
        WorkloadKind::HomebrewAudit => Vec::new(),
        WorkloadKind::PackageUpdateFixture => {
            return Err(crate::internal::internal(
                "package_update_source_bound_fixture_pending",
            ));
        }
        WorkloadKind::SwiftTest => vec![
            command("build", &["swift", "build"]),
            command("test", &["swift", "test", "--parallel"]),
        ],
        WorkloadKind::GradleCheck | WorkloadKind::GradleDatabaseCheck => Vec::new(),
        WorkloadKind::Reuse => vec![("reuse", velnor_actions_native::reuse::lint())],
        WorkloadKind::RubySyntax => vec![(
            "syntax",
            velnor_actions_native::ruby::syntax(&relative_files(workload))?,
        )],
        kind @ (WorkloadKind::CargoAudit
        | WorkloadKind::CargoDeny
        | WorkloadKind::Alint
        | WorkloadKind::TuiNoDefaultGraph
        | WorkloadKind::TuiXtaskPolicy
        | WorkloadKind::TuiXtaskDeps
        | WorkloadKind::TuiXtaskPackage) => security::phases(kind),
        WorkloadKind::Shellcheck => vec![(
            "shellcheck",
            velnor_actions_native::shell::check(&relative_files(workload))?,
        )],
    };
    Ok(phases)
}

fn relative_files(workload: &WorkloadConfig) -> Vec<String> {
    let prefix = format!("{}/", workload.root.as_str());
    workload
        .paths
        .iter()
        .map(|path| path.strip_prefix(&prefix).unwrap_or(path).to_owned())
        .collect()
}

/// Stable closed kind spelling carried by proposals and tool identity.
pub(crate) const fn kind_id(kind: WorkloadKind) -> &'static str {
    match kind {
        WorkloadKind::DockerBuild => "docker_build",
        WorkloadKind::BunCi => "bun_ci",
        WorkloadKind::NodeCi => "node_ci",
        WorkloadKind::SwiftTest => "swift_test",
        kind @ (WorkloadKind::NativeXcodeProjectCi | WorkloadKind::NativeSwiftPackageCi) => {
            desktop::kind_id(kind)
        }
        WorkloadKind::HomebrewAudit => "homebrew_audit",
        WorkloadKind::PackageUpdateFixture => "package_update_fixture",
        WorkloadKind::RubySyntax => "ruby_syntax",
        WorkloadKind::Shellcheck => "shellcheck",
        WorkloadKind::Reuse => "reuse",
        WorkloadKind::GradleCheck => "gradle_check",
        WorkloadKind::GradleDatabaseCheck => "gradle_database_check",
        WorkloadKind::CargoAudit => "cargo_audit",
        WorkloadKind::CargoDeny => "cargo_deny",
        WorkloadKind::Alint => "alint",
        WorkloadKind::TuiNoDefaultGraph => "tui_no_default_graph",
        WorkloadKind::TuiXtaskPolicy => "tui_xtask_policy",
        WorkloadKind::TuiXtaskDeps => "tui_xtask_deps",
        WorkloadKind::TuiXtaskPackage => "tui_xtask_package",
    }
}

/// Catalog-owned tools for the closed workload configuration.
pub(crate) fn tools(kind: &str) -> Result<Vec<PinnedTool>, ContractError> {
    Ok(match kind {
        "docker_build" => Vec::new(),
        "bun_ci" => vec![PinnedTool::Bun],
        "node_ci" => vec![PinnedTool::Node],
        "swift_test" => vec![PinnedTool::Swift],
        "native_xcode_project_ci" => desktop::tools(WorkloadKind::NativeXcodeProjectCi),
        "native_swift_package_ci" => desktop::tools(WorkloadKind::NativeSwiftPackageCi),
        "ruby_syntax" => vec![PinnedTool::Ruby],
        "package_update_fixture" => vec![PinnedTool::Ruby, PinnedTool::Jq],
        "shellcheck" => vec![PinnedTool::Shellcheck],
        "reuse" => vec![PinnedTool::Python, PinnedTool::Uv, PinnedTool::Reuse],
        "gradle_check" | "gradle_database_check" => vec![PinnedTool::Java],
        "cargo_audit"
        | "cargo_deny"
        | "alint"
        | "tui_no_default_graph"
        | "tui_xtask_policy"
        | "tui_xtask_deps"
        | "tui_xtask_package" => security::tools(kind)?,
        _ => {
            return Err(ContractError::identity(
                "workload_kind",
                "unknown_workload_kind",
            ));
        }
    })
}

/// Closed phase order shared by native obligation jobs.
pub(crate) fn rank(kind: &str) -> u32 {
    if let Some(rank) = desktop::rank(kind) {
        return rank;
    }
    if let Some(rank) = velnor_actions_native::node::rank(kind) {
        return rank;
    }
    match kind {
        "install" | "config" => 0,
        "syntax" | "shellcheck" | "reuse" | "alint" => 1,
        "build" => 2,
        "test" => 3,
        _ => 4,
    }
}

/// Human names stay independent from task identifiers.
pub(crate) fn step_name(kind: &str) -> &'static str {
    if let Some(name) = desktop::step_name(kind) {
        return name;
    }
    if let Some(name) = velnor_actions_native::node::step_name(kind) {
        return name;
    }
    match kind {
        "install" => "Verify dependencies",
        "build" => "Build",
        "test" => "Tests",
        "syntax" => "Ruby syntax",
        "shellcheck" => "Shellcheck",
        "reuse" => "REUSE compliance",
        "config" => "Validate Alint configuration",
        "alint" => "Repository shape and line limits",
        "audit" => "Fresh dependency advisory audit",
        "deny" => "Dependency policy",
        "graph" => "No-default dependency graph",
        "policy" => "Source policy",
        "deps" => "Dependency inspection",
        "package" => "Package contents",
        _ => "Native validation",
    }
}
