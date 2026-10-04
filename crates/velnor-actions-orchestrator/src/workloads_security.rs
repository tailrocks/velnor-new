//! Fixed security and reviewed repository-policy operations; no command override.

use velnor_actions_contract::ContractError;
use velnor_actions_contract::FileIndex;
use velnor_actions_contract::config::{WorkloadConfig, WorkloadKind};
use velnor_actions_mise::PinnedTool;

/// Cargo obligations use isolated Rust homes even though they are native workloads.
pub(crate) fn requires_rust(kind: &str) -> bool {
    matches!(
        kind,
        "cargo_audit"
            | "cargo_deny"
            | "tui_no_default_graph"
            | "tui_xtask_policy"
            | "tui_xtask_deps"
            | "tui_xtask_package"
    )
}

/// Locked Cargo policy may never silently degrade to an unlocked check.
pub(super) fn validate_evidence(
    workload: &WorkloadConfig,
    index: &FileIndex,
) -> Result<(), crate::OrchestratorError> {
    if !requires_rust(super::kind_id(workload.kind)) {
        return Ok(());
    }
    let root = workload.root.as_str();
    let lock = if root == "." {
        "Cargo.lock".to_owned()
    } else {
        format!("{root}/Cargo.lock")
    };
    if !index.contains(&lock) {
        return Err(crate::internal::internal(&format!(
            "security_lock_missing:{lock}"
        )));
    }
    Ok(())
}

pub(super) fn tools(kind: &str) -> Result<Vec<PinnedTool>, ContractError> {
    Ok(match kind {
        "cargo_audit" => vec![PinnedTool::Rust, PinnedTool::CargoAudit],
        "cargo_deny" => vec![PinnedTool::Rust, PinnedTool::CargoDeny],
        "alint" => vec![PinnedTool::Alint],
        kind if requires_rust(kind) => vec![PinnedTool::Rust],
        _ => {
            return Err(ContractError::identity(
                "workload_kind",
                "unknown_security_kind",
            ));
        }
    })
}

pub(super) fn phases(kind: WorkloadKind) -> Vec<(&'static str, Vec<String>)> {
    let command =
        |phase, args: &[&str]| (phase, args.iter().map(|arg| (*arg).to_owned()).collect());
    match kind {
        WorkloadKind::CargoAudit => vec![command("audit", &["cargo", "audit"])],
        WorkloadKind::CargoDeny => vec![command("deny", &["cargo", "deny", "check", "--locked"])],
        WorkloadKind::Alint => vec![
            command("config", &["alint", "validate-config"]),
            command("alint", &["alint", "check"]),
        ],
        WorkloadKind::TuiNoDefaultGraph => vec![command(
            "graph",
            &[
                "sh",
                "-c",
                "set -eu; mkdir -p \"$RUNNER_TEMP/velnor/graph\"; cargo tree -p tuiscotti --no-default-features --locked --prefix none > \"$RUNNER_TEMP/velnor/graph/no-default.txt\"; awk '/^(portable-pty|alacritty_terminal|termpane)( |$)/ { exit 1 } NF { seen = 1 } END { if (!seen) exit 1 }' \"$RUNNER_TEMP/velnor/graph/no-default.txt\"; rm \"$RUNNER_TEMP/velnor/graph/no-default.txt\"",
            ],
        )],
        WorkloadKind::TuiXtaskPolicy => xtask("policy"),
        WorkloadKind::TuiXtaskDeps => xtask("deps"),
        WorkloadKind::TuiXtaskPackage => xtask("package"),
        _ => Vec::new(),
    }
}

fn xtask(operation: &'static str) -> Vec<(&'static str, Vec<String>)> {
    vec![(
        operation,
        ["cargo", "run", "--locked", "-p", "xtask", "--", operation]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_security_checks_preserve_fixed_commands() {
        assert_eq!(phases(WorkloadKind::CargoAudit)[0].1, ["cargo", "audit"]);
        assert_eq!(
            phases(WorkloadKind::CargoDeny)[0].1,
            ["cargo", "deny", "check", "--locked"]
        );
        assert_eq!(phases(WorkloadKind::Alint).len(), 2);
        assert!(requires_rust("cargo_deny"));
        assert!(!requires_rust("alint"));
        assert!(tools("unreviewed").is_err());
    }

    #[test]
    fn live_advisories_never_become_covered_pass_results() {
        for kind in [WorkloadKind::CargoAudit, WorkloadKind::CargoDeny] {
            let workload = WorkloadConfig {
                name: "security".to_owned(),
                kind,
                root: velnor_actions_contract::config::Utf8RepoRelDir::from_raw(".".to_owned()),
                inputs: Vec::new(),
                paths: Vec::new(),
                scripts: None,
                gradle: None,
                package_update: None,
                native_desktop: None,
            };
            let (phase, payload) = phases(kind).remove(0);
            let task = super::super::proposal(&workload, phase, payload, None);
            assert!(!task.cache_policy.allow_task_reuse);
            assert!(!task.cache_policy.allow_compilation_reuse);
            assert!(task.resource.needs_network);
            assert!(task.identity.undeclared_reads);
            assert!(task.uses_clock);
            assert!(!task.payload.iter().any(|arg| arg == "--offline"));
            assert!(task.validate().is_ok());
        }
    }

    #[test]
    fn reviewed_xtask_variants_have_no_arguments_escape_hatch() {
        for (kind, operation) in [
            (WorkloadKind::TuiXtaskPolicy, "policy"),
            (WorkloadKind::TuiXtaskDeps, "deps"),
            (WorkloadKind::TuiXtaskPackage, "package"),
        ] {
            assert_eq!(
                phases(kind)[0].1,
                ["cargo", "run", "--locked", "-p", "xtask", "--", operation]
            );
        }
        let graph = &phases(WorkloadKind::TuiNoDefaultGraph)[0].1;
        assert!(graph[2].contains("set -eu"));
        assert!(!graph[2].contains("$("));
        assert!(velnor_actions_workflow_renderer::validate_command_argv(graph).is_ok());
        assert!(graph[2].contains("--locked"));
        assert!(graph[2].contains("portable-pty|alacritty_terminal|termpane"));
    }
}
