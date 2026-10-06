use velnor_actions_rust::{
    CompileDriver, NextestProfile, PackageRecord, ProfileSource, RustExecutionProfile, TestRunner,
    WorkspaceRecord,
};

use super::*;
use crate::discover::PlannedWorkspace;
use crate::select::classify_changed;

/// Discovery with packages `a` and `b` under `a/` and `b/`.
fn two_package_discovery() -> Discovery {
    let package = |id: &str| PackageRecord {
        id: id.to_owned(),
        name: id.to_owned(),
        version: "0.1.0".to_owned(),
        manifest: format!("{id}/Cargo.toml"),
        external: false,
        in_workspace: true,
        targets: Vec::new(),
        features: Vec::new(),
        has_build_script: false,
    };
    Discovery {
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        feature_fallbacks: Vec::new(),
        workspaces: vec![PlannedWorkspace {
            record: WorkspaceRecord {
                workspace_root: String::new(),
                members: vec!["a".to_owned(), "b".to_owned()],
                packages: vec![package("a"), package("b")],
                edges: Vec::new(),
                skipped_edges: Vec::new(),
            },
            profile: RustExecutionProfile {
                compile_driver: CompileDriver::Cargo,
                test_runner: TestRunner::CargoTest,
                evidence: Vec::new(),
                driver_source: ProfileSource::Detected,
                runner_source: ProfileSource::Detected,
                nextest_profile: NextestProfile::Default,
                nextest_config: None,
                run_ignored: None,
            },
            recommendations: Vec::new(),
            findings: Vec::new(),
        }],
        proposals: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: crate::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

fn edge(from: &str, to: &str) -> (String, String) {
    (from.to_owned(), to.to_owned())
}

/// Discovery with the given `(id, manifest)` packages in one workspace.
fn discovery_with(packages: &[(&str, &str)]) -> Discovery {
    let mut discovery = two_package_discovery();
    discovery.workspaces[0].record.packages = packages
        .iter()
        .map(|(id, manifest)| PackageRecord {
            id: (*id).to_owned(),
            name: (*id).to_owned(),
            version: "0.1.0".to_owned(),
            manifest: (*manifest).to_owned(),
            external: false,
            in_workspace: true,
            targets: Vec::new(),
            features: Vec::new(),
            has_build_script: false,
        })
        .collect();
    discovery.workspaces[0].record.members =
        packages.iter().map(|(id, _)| (*id).to_owned()).collect();
    discovery
}

mod select_affected_tests;
