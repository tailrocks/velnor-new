//! Coverage test fixtures: plans, manifests, discovery, closures.
//!
//! Declared via `#[path]` from `cover_identity.rs` under `cfg(test)`;
//! helpers stay here so the test module keeps the file size gate.

use super::*;
use crate::merge::required_evidence::BaselineTaskEntry;
use velnor_actions_contract::{
    PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, RunnerSelection, Trust,
    WorkflowEvent, digest_b3,
};
use velnor_actions_mise::ToolCatalog;

/// Plan carrying one execute obligation per `task_ids`.
pub(super) fn plan_with(task_ids: &[&str]) -> Plan {
    let digest = digest_b3(b"digest");
    Plan {
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: "plan-local".to_owned(),
        base: None,
        head: "head".to_owned(),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(None).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "1".repeat(64),
        },
        packages: Vec::new(),
        obligations: task_ids
            .iter()
            .map(|id| PlanObligation {
                task_id: (*id).to_owned(),
                decision: ObligationDecision::Execute,
                reason: "selected".to_owned(),
                task_digest: digest.clone(),
                input_digest: digest.clone(),
                closure_digest: digest.clone(),
                baseline_proof: None,
            })
            .collect(),
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids: task_ids.iter().map(|id| (*id).to_owned()).collect(),
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

/// Manifest binding every `(task_id, closure_digest)` entry.
pub(super) fn manifest_with(entries: &[(&str, &str)]) -> BaselineManifest {
    let digest = digest_b3(b"digest");
    BaselineManifest {
        schema: 2,
        repository_id: digest.clone(),
        source_commit: "a".repeat(40),
        ref_: "refs/heads/testmain".to_owned(),
        event: "push".to_owned(),
        workflow_ref: "o/r/.github/workflows/ci.yml@refs/heads/testmain".to_owned(),
        run_id: 7,
        run_attempt: 1,
        final_status: "passed".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        compatibility_id: digest.clone(),
        artifact_id: 9,
        artifact_name: "velnor-baseline".to_owned(),
        expires_at_unix: None,
        tasks: entries
            .iter()
            .map(|(id, closure)| BaselineTaskEntry {
                task_id: (*id).to_owned(),
                task_digest: digest.clone(),
                input_digest: digest.clone(),
                closure_digest: (*closure).to_owned(),
                proof_run_id: 7,
                observed_run_id: 7,
                external_data: None,
                proof: None,
            })
            .collect(),
    }
}

/// Discovery with one plain group per task ID, all unchanged.
pub(super) fn discovery_with(task_ids: &[&str]) -> Discovery {
    use velnor_actions_rust::{TaskGroup, TaskKind};
    Discovery {
        statuses: Vec::new(),
        workspaces: Vec::new(),
        task_groups: task_ids
            .iter()
            .map(|id| TaskGroup {
                task_id: (*id).to_owned(),
                package_id: "demo".to_owned(),
                package_name: "demo".to_owned(),
                manifest_key: "root".to_owned(),
                kind: TaskKind::Clippy,
                configuration: "default".to_owned(),
                features: Vec::new(),
                target: "host".to_owned(),
                gated_by: Vec::new(),
                depends_on: Vec::new(),
                target_flags: Vec::new(),
                no_test_targets: false,
                package_arg: None,
                compile_driver: "cargo".to_owned(),
                test_runner: "cargo_test".to_owned(),
                declared_inputs: Vec::new(),
                undeclared_reads: false,
                uses_network: false,
                uses_clock: false,
                uses_random: false,
                nextest_profile: "default".to_owned(),
            })
            .collect(),
        tool_checks: Vec::new(),
        clippy_memory: crate::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
    }
}

/// Baseline inputs over a temp checkout root.
pub(super) fn inputs<'a>(
    root: &'a std::path::Path,
    catalog: &'a ToolCatalog,
) -> BaselineInputs<'a> {
    BaselineInputs {
        branch: "testmain",
        root,
        workflow: ".github/workflows/ci.yml",
        catalog,
    }
}

/// Validated provenance for coverage tests.
pub(super) fn provenance() -> ValidatedProvenance {
    ValidatedProvenance {
        source_commit: "a".repeat(40),
        run_id: 7,
        artifact_name: format!("velnor-baseline-{}-{}", "a".repeat(40), digest_b3(b"m")),
        artifact_id: 9,
        manifest_digest: digest_b3(b"m"),
    }
}

/// Placeholder closure digest for entries that refuse before comparison.
pub(super) fn stale_closure() -> String {
    digest_b3(b"stale-closure")
}

/// Source files backing a content-bound closure in `root`.
pub(super) fn seed_sources(root: &std::path::Path) {
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("manifest");
    std::fs::create_dir(root.join("src")).expect("src");
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\n").expect("source");
}

/// Live closure digest for `task_id`, resolved exactly like cover does.
pub(super) fn live_closure_digest(
    root: &std::path::Path,
    discovery: &Discovery,
    task_id: &str,
    catalog: &ToolCatalog,
) -> String {
    let group = discovery
        .task_groups
        .iter()
        .find(|group| group.task_id == task_id)
        .expect("group");
    let snapshot = ExecutionSnapshot::build(discovery);
    let bundle = extension_bundle_with_snapshot(
        &snapshot,
        discovery,
        group,
        Some(root),
        nextest_config_for(discovery, group).as_deref(),
    );
    let toolchain = toolchain_id(group, catalog).expect("toolchain");
    let platform = platform_id_for_group("ubuntu-26.04", group);
    let closure = resolve_closure_at_root(
        root,
        group,
        nextest_config_for(discovery, group).as_deref(),
        bundle.graph_digest(),
        &toolchain,
        &platform,
    );
    canonical_digest(&closure).expect("digest")
}
