//! Coverage test fixtures: plans, manifests, discovery, closures.
//!
//! Declared via `#[path]` from `cover_identity.rs` under `cfg(test)`;
//! helpers stay here so the test module keeps the file size gate.

use super::*;
use crate::cover_baseline::provenance_check::{
    ProvenanceExpectations, ValidatedProvenance, validate_provenance,
};
use crate::merge::required_evidence::BaselineTaskEntry;
use velnor_actions_contract::{
    PlanBaseline, PlanGenerator, PlanMatrix, PlanObligation, PlanRunner, RunnerSelection, Trust,
    WorkflowEvent, canonical_json_bytes, digest_b3,
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
///
/// The manifest validates end to end: source commit, artifact name,
/// run binding, generator, and per-task identities all satisfy
/// [`validate_provenance`], so fixture coverage never runs on evidence
/// production would reject.
pub(super) fn manifest_with(entries: &[(&str, &str)]) -> BaselineManifest {
    let digest = digest_b3(b"digest");
    let commit = "a".repeat(40);
    let name = format!("velnor-baseline-{commit}-{digest}");
    BaselineManifest {
        schema: 2,
        repository_id: digest_b3("github.com/o/r".as_bytes()),
        source_commit: commit.clone(),
        ref_: "refs/heads/testmain".to_owned(),
        event: "push".to_owned(),
        workflow_ref: "o/r/.github/workflows/ci.yml@refs/heads/testmain".to_owned(),
        run_id: 7,
        run_attempt: 1,
        final_status: "passed".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        compatibility_id: digest.clone(),
        artifact_id: crate::cover_compat::baseline_artifact_numeric_id(&name),
        artifact_name: name,
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

/// Discovery with one plain proposal per task ID, all unchanged.
pub(super) fn discovery_with(task_ids: &[&str]) -> Discovery {
    use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};
    Discovery {
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals: task_ids
            .iter()
            .map(|id| {
                let group = TaskGroup {
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
                    compile_driver: CompileDriver::Cargo,
                    test_runner: TestRunner::CargoTest,
                    declared_inputs: Vec::new(),
                    undeclared_reads: false,
                    uses_network: false,
                    uses_clock: false,
                    uses_random: false,
                    run_ignored: None,
                    nextest_profile: NextestProfile::Default,
                };
                let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
                task.validate().expect("fixture valid");
                task
            })
            .collect(),
        feature_fallbacks: Vec::new(),
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
        repository: None,
    }
}

/// Validated provenance for a fixture manifest, through real validation.
///
/// Coverage tests never run on hand-built provenance: the manifest
/// must validate exactly like production evidence, and the digest
/// binds its canonical bytes.
pub(super) fn provenance_for(manifest: &BaselineManifest) -> ValidatedProvenance {
    let expected = ProvenanceExpectations {
        base: "a".repeat(40),
        branch: "testmain".to_owned(),
        workflow_path: ".github/workflows/ci.yml".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        repository_id: Some(digest_b3("github.com/o/r".as_bytes())),
        repository_slug: Some("o/r".to_owned()),
        repository_conflict: false,
    };
    let bytes = canonical_json_bytes(manifest).expect("canonical");
    let digest = digest_b3(&bytes);
    validate_provenance(manifest, &digest, &expected).expect("provenance")
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
    let task = discovery
        .proposals
        .iter()
        .find(|task| task.task_id == task_id)
        .expect("task");
    let snapshot = ExecutionSnapshot::build(discovery);
    let bundle = extension_bundle_with_snapshot(
        &snapshot,
        discovery,
        task,
        Some(root),
        nextest_config_for(discovery, task).as_deref(),
    );
    let toolchain = toolchain_id(task, catalog).expect("toolchain");
    let platform = platform_id_for_group("ubuntu-26.04", task).expect("platform");
    let closure = resolve_closure_at_root(
        root,
        task,
        nextest_config_for(discovery, task).as_deref(),
        bundle.graph_digest(),
        &toolchain,
        &platform,
        &mut velnor_actions_tofu::FileCache::new(),
    )
    .expect("closure");
    canonical_digest(&closure).expect("digest")
}
