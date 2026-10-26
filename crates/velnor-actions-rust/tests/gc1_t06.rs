// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_rust::identity::{
    DependencyCoverage, DependencyCoverageInputs, dependency_coverage,
};
use velnor_actions_rust::metadata::{PackageRecord, VelnorV1TaskOwner, WorkspaceRecord};
use velnor_actions_rust::profile::{CompileDriver, NextestProfile, TestRunner};
use velnor_actions_rust::resolved_graph::{ResolvedGraph, ResolvedPathPackage};
use velnor_actions_rust::tasks::TaskKind;
use velnor_actions_rust::{TaskGroup, identity::DigestSlot};

const APP: &str = "workspace-app";
const LIB: &str = "workspace-lib";
const SERDE: &str = "registry-serde";

fn digest_fill(fill: char) -> String {
    format!("b3-{}", fill.to_string().repeat(64))
}

fn fixture_graph() -> ResolvedGraph {
    ResolvedGraph {
        edges: vec![
            (APP.to_owned(), LIB.to_owned()),
            (APP.to_owned(), SERDE.to_owned()),
            (LIB.to_owned(), SERDE.to_owned()),
        ],
        path_packages: vec![
            ResolvedPathPackage {
                id: APP.to_owned(),
                dir: "crates/app".to_owned(),
            },
            ResolvedPathPackage {
                id: LIB.to_owned(),
                dir: "crates/lib".to_owned(),
            },
        ],
    }
}

fn fixture_package(id: &str, name: &str, version: &str, manifest: &str) -> PackageRecord {
    PackageRecord {
        id: id.to_owned(),
        name: name.to_owned(),
        version: version.to_owned(),
        manifest: manifest.to_owned(),
        external: false,
        in_workspace: true,
        targets: Vec::new(),
        features: vec!["serde".to_owned()],
        has_build_script: false,
        v1_task_owner: VelnorV1TaskOwner::Project,
    }
}

fn fixture_workspaces(app_version: &str, lib_version: &str) -> Vec<WorkspaceRecord> {
    vec![WorkspaceRecord {
        workspace_root: String::new(),
        members: vec![APP.to_owned(), LIB.to_owned()],
        packages: vec![
            fixture_package(APP, "app", app_version, "crates/app/Cargo.toml"),
            fixture_package(LIB, "lib", lib_version, "crates/lib/Cargo.toml"),
        ],
        edges: Vec::new(),
        skipped_edges: Vec::new(),
        has_external_dependencies: true,
    }]
}

fn fixture_inputs<'a>(
    graph: &'a ResolvedGraph,
    workspaces: &'a [WorkspaceRecord],
    lock: &'a DigestSlot,
    features: &'a [String],
    targets: &'a [String],
    native_inputs: &'a [String],
) -> DependencyCoverageInputs<'a> {
    DependencyCoverageInputs {
        package_id: APP,
        graph,
        workspaces,
        features,
        all_features: false,
        targets,
        lock_digest: lock,
        native_inputs,
    }
}

fn base_setup() -> (
    ResolvedGraph,
    Vec<WorkspaceRecord>,
    DigestSlot,
    Vec<String>,
    Vec<String>,
    Vec<String>,
) {
    (
        fixture_graph(),
        fixture_workspaces("0.1.0", "0.2.0"),
        DigestSlot::Known(digest_fill('f')),
        vec!["serde".to_owned()],
        vec!["x86_64-unknown-linux-gnu".to_owned()],
        vec!["openssl-src".to_owned()],
    )
}

fn base_coverage() -> Result<DependencyCoverage, Box<dyn Error>> {
    let (graph, workspaces, lock, features, targets, native_inputs) = base_setup();
    Ok(dependency_coverage(&fixture_inputs(
        &graph,
        &workspaces,
        &lock,
        &features,
        &targets,
        &native_inputs,
    ))?)
}

fn fixture_group() -> TaskGroup {
    TaskGroup {
        task_id: "rust-app-build".to_owned(),
        package_id: APP.to_owned(),
        package_name: "app".to_owned(),
        manifest_key: "crates/app/Cargo.toml".to_owned(),
        kind: TaskKind::Build,
        configuration: "dev".to_owned(),
        features: vec!["serde".to_owned()],
        all_features: false,
        target: "x86_64-unknown-linux-gnu".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        run_ignored: None,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    }
}

mod preserves;
mod rejects;
