//! Authoritative selected workspace roots for one compiler lane.

use std::collections::BTreeSet;

use velnor_actions_contract::{ProposedTask, Stack};

use crate::discover::Discovery;

/// Select locked roots using Cargo package identities, never path heuristics.
///
/// Unknown or ambiguous identity-to-workspace mapping uses every detected
/// locked root. Cargo fetch then covers the complete locked workspace because
/// the pinned CLI has no package/feature narrowing. A source snapshot from
/// the Plan's containing root universe remains a compatible restore fallback.
pub(crate) fn selected_fetch_roots(
    discovery: &Discovery,
    tasks: &[&ProposedTask],
    roots: &[String],
) -> Vec<String> {
    let mut selected = BTreeSet::new();
    for task in tasks
        .iter()
        .filter(|task| task.stack_id == Stack::Rust.id())
    {
        let mut matches = discovery
            .workspaces
            .iter()
            .filter(|workspace| workspace.record.members.contains(&task.identity.unit_id));
        let Some(workspace) = matches.next() else {
            return roots.to_vec();
        };
        if matches.next().is_some() {
            return roots.to_vec();
        }
        if roots.contains(&workspace.record.workspace_root) {
            selected.insert(workspace.record.workspace_root.clone());
        }
    }
    selected.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clippy_groups::ClippyMemoryPlan;
    use crate::discover::PlannedWorkspace;
    use velnor_actions_rust::{
        CompileDriver, NextestProfile, ProfileSource, RustExecutionProfile, TestRunner,
        WorkspaceRecord,
    };

    fn profile() -> RustExecutionProfile {
        RustExecutionProfile {
            compile_driver: CompileDriver::Cargo,
            test_runner: TestRunner::CargoTest,
            evidence: Vec::new(),
            driver_source: ProfileSource::Detected,
            runner_source: ProfileSource::Detected,
            nextest_profile: NextestProfile::Default,
            nextest_config: None,
        }
    }

    fn workspace(root: &str, member: &str) -> PlannedWorkspace {
        PlannedWorkspace {
            record: WorkspaceRecord {
                workspace_root: root.to_owned(),
                members: vec![member.to_owned()],
                packages: Vec::new(),
                edges: Vec::new(),
                skipped_edges: Vec::new(),
            },
            profile: profile(),
            recommendations: Vec::new(),
            findings: Vec::new(),
        }
    }

    fn discovery(workspaces: Vec<PlannedWorkspace>) -> Discovery {
        Discovery {
            rust_inventory: None,
            raw_inventories: Vec::new(),
            statuses: Vec::new(),
            workspaces,
            proposals: Vec::new(),
            feature_fallbacks: Vec::new(),
            tool_checks: Vec::new(),
            clippy_memory: ClippyMemoryPlan {
                groups: Vec::new(),
                barriers: 0,
            },
            recommendations: Vec::new(),
            consumer_manifest_json: None,
            consumer_manifest_stand_in: false,
            skipped_non_utf8: false,
            tofu_note: None,
            tofu_units: Vec::new(),
        }
    }

    fn task(member: &str) -> ProposedTask {
        let group = velnor_actions_rust::derive_workspace_fmt(
            "misleading/Cargo.toml",
            &profile(),
            "default",
            "host",
        )
        .expect("fixture format group");
        let mut proposal = velnor_actions_rust::propose_task(&group).expect("fixture task");
        proposal.identity.unit_id = member.to_owned();
        proposal
    }

    #[test]
    fn authoritative_member_identity_selects_its_locked_workspace() {
        let found = discovery(vec![
            workspace("alpha", "package-alpha"),
            workspace("beta", "package-beta"),
        ]);
        let roots = vec!["alpha".to_owned(), "beta".to_owned()];
        let selected = task("package-beta");
        assert_eq!(selected_fetch_roots(&found, &[&selected], &roots), ["beta"]);
    }

    #[test]
    fn unknown_member_after_known_member_broadens_to_all_locked_roots() {
        let found = discovery(vec![
            workspace("alpha", "package-alpha"),
            workspace("beta", "package-beta"),
        ]);
        let roots = vec!["alpha".to_owned(), "beta".to_owned()];
        let known = task("package-alpha");
        let unknown = task("package-missing");
        assert_eq!(
            selected_fetch_roots(&found, &[&known, &unknown], &roots),
            roots
        );
    }

    #[test]
    fn ambiguous_member_identity_broadens_to_all_locked_roots() {
        let found = discovery(vec![
            workspace("alpha", "same-package"),
            workspace("beta", "same-package"),
            workspace("gamma", "other-package"),
        ]);
        let roots = vec!["alpha".to_owned(), "beta".to_owned(), "gamma".to_owned()];
        let selected = task("same-package");
        assert_eq!(selected_fetch_roots(&found, &[&selected], &roots), roots);
    }
}
