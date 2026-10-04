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
use velnor_actions_mise::{RuntimePaths, ToolCatalog};

/// Plan carrying one execute obligation per `task_ids`.
pub(super) fn plan_with(task_ids: &[&str]) -> Plan {
    let digest = digest_b3(b"digest");
    Plan {
        producers: Default::default(),
        schema: 1,
        run_key: "local".to_owned(),
        plan_id: "plan-local".to_owned(),
        base: None,
        head: "head".to_owned(),
        event: WorkflowEvent::PullRequest,
        scope: velnor_actions_contract::VerificationScope::Affected,
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
                job_id: "rust-demo".to_owned(),
                decision: ObligationDecision::Execute,
                reason: "selected".to_owned(),
                task_digest: digest.clone(),
                input_digest: digest.clone(),
                closure_digest: digest.clone(),
                execution_identity: fixture_execution_identity(id),
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
        schema: 3,
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
        parent: None,
        expires_at_unix: None,
        tasks: entries
            .iter()
            .map(|(id, closure)| BaselineTaskEntry {
                task_id: (*id).to_owned(),
                task_digest: digest.clone(),
                input_digest: digest.clone(),
                closure_digest: (*closure).to_owned(),
                proof_run_id: 7,
                carried_from: None,
                observed_run_id: 7,
                external_data: None,
                proof: Some(fixture_task_proof(id, &digest)),
            })
            .collect(),
    }
}

/// Baseline dimensions matching the ordinary Rust proposal fixture.
fn fixture_execution_identity(id: &str) -> velnor_actions_contract::TaskExecutionIdentity {
    let discovery = discovery_with(&[id]);
    let task = &discovery.proposals[0];
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = extension_bundle_with_snapshot(&snapshot, &discovery, task, None, None);
    let catalog = ToolCatalog::pinned();
    crate::internal_plan::identities::execution_identity_for(
        task,
        &bundle,
        &catalog,
        &toolchain_id_for_runner(task, &catalog, "ubuntu-26.04").expect("toolchain"),
        &platform_id_for_group("ubuntu-26.04", task).expect("platform"),
    )
    .expect("execution identity")
}

/// Structured proof bound to the same ordinary execution dimensions.
fn fixture_task_proof(id: &str, digest: &str) -> ManifestTaskProof {
    let identity = fixture_execution_identity(id);
    ManifestTaskProof::new(
        id,
        digest,
        digest,
        identity.graph_digest(),
        identity.toolchain_id(),
        identity.mbx_digest(),
        identity.platform_id(),
        identity.profile(),
        7,
    )
    .expect("task proof")
}

/// Discovery with one plain proposal per task ID, all unchanged.
pub(crate) fn discovery_with(task_ids: &[&str]) -> Discovery {
    use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};
    Discovery {
        rust_inventory: None,
        raw_inventories: Vec::new(),
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
        consumer_manifest_stand_in: false,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

/// Baseline inputs over a temp checkout root.
pub(crate) fn inputs<'a>(
    root: &'a std::path::Path,
    catalog: &'a ToolCatalog,
) -> BaselineInputs<'a> {
    BaselineInputs {
        branch: "testmain",
        root,
        workflow: ".github/workflows/ci.yml",
        catalog,
        repository: None,
        runtime: RuntimePaths::full(),
    }
}

/// Validated provenance for a fixture manifest, through real validation.
///
/// Coverage tests never run on hand-built provenance: the manifest
/// must validate exactly like production evidence, and the digest
/// binds its canonical bytes.
pub(crate) fn provenance_for(manifest: &BaselineManifest) -> ValidatedProvenance {
    let expected = ProvenanceExpectations {
        base: manifest.source_commit.clone(),
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
pub(crate) fn seed_sources(root: &std::path::Path) {
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
    let toolchain = toolchain_id_for_runner(task, catalog, "ubuntu-26.04").expect("toolchain");
    let platform = platform_id_for_group("ubuntu-26.04", task).expect("platform");
    let closure = resolve_closure_at_root(
        root,
        task,
        nextest_config_for(discovery, task).as_deref(),
        bundle.graph_digest(),
        &toolchain,
        &platform,
        &mut velnor_actions_tofu::FileCache::new(),
        None,
    )
    .expect("closure");
    canonical_digest(&closure).expect("digest")
}

/// Conservative fixture classification; refinement needs separate qualification.
pub(super) fn selection(ids: &[&str]) -> crate::select::ChangedSelection {
    crate::select::ChangedSelection {
        affected: ids.iter().map(|id| (*id).to_owned()).collect(),
        proof_refinable: Default::default(),
    }
}

/// Original checkout evidence retained while a real Git fixture changes topology.
pub(crate) struct OriginalBaseline {
    manifest: BaselineManifest,
    task_id: String,
}

impl OriginalBaseline {
    pub(crate) fn capture(root: &std::path::Path, discovery: &Discovery, unit_path: &str) -> Self {
        let task = discovery
            .proposals
            .iter()
            .find(|task| task.identity.unit_path == unit_path && task.task_kind == "clippy")
            .expect("original clippy");
        let catalog = ToolCatalog::pinned();
        let live = live_closure_digest(root, discovery, &task.task_id, &catalog);
        let snapshot = ExecutionSnapshot::build(discovery).with_checkout(root);
        let bundle = extension_bundle_with_snapshot(&snapshot, discovery, task, Some(root), None);
        let mut manifest = manifest_with(&[(&task.task_id, &live)]);
        let commit = velnor_actions_mise::GitRequest::rev_parse(vec!["HEAD".into()])
            .run_in(root)
            .expect("original commit");
        assert!(commit.success, "original checkout must be committed");
        manifest.source_commit = String::from_utf8(commit.stdout)
            .expect("commit UTF8")
            .trim()
            .to_owned();
        let digest = digest_b3(b"digest");
        manifest.artifact_name = format!("velnor-baseline-{}-{digest}", manifest.source_commit);
        manifest.artifact_id =
            crate::cover_compat::baseline_artifact_numeric_id(&manifest.artifact_name);
        manifest.tasks[0].proof = Some(
            ManifestTaskProof::new(
                &task.task_id,
                &digest,
                &digest,
                bundle.graph_digest(),
                &toolchain_id_for_runner(task, &catalog, "ubuntu-26.04").expect("toolchain"),
                &live_mbx_digest(task, &catalog),
                &platform_id_for_group("ubuntu-26.04", task).expect("platform"),
                &task.configuration,
                7,
            )
            .expect("original proof"),
        );
        let _verified = provenance_for(&manifest);
        Self {
            manifest,
            task_id: task.task_id.clone(),
        }
    }

    pub(crate) fn check(
        &self,
        root: &std::path::Path,
        discovery: &Discovery,
        changed: &crate::select::ChangedSelection,
    ) -> (u32, Vec<String>) {
        let catalog = ToolCatalog::pinned();
        // Keep original task/input identities equal to isolate the live proof/closure gate.
        let mut plan = plan_with(&[&self.task_id]);
        if let Some(task) = discovery
            .proposals
            .iter()
            .find(|task| task.task_id == self.task_id)
        {
            let snapshot = ExecutionSnapshot::build(discovery).with_checkout(root);
            let bundle =
                extension_bundle_with_snapshot(&snapshot, discovery, task, Some(root), None);
            plan.obligations[0].execution_identity =
                crate::internal_plan::identities::execution_identity_for(
                    task,
                    &bundle,
                    &catalog,
                    &toolchain_id_for_runner(task, &catalog, "ubuntu-26.04").expect("toolchain"),
                    &platform_id_for_group("ubuntu-26.04", task).expect("platform"),
                )
                .expect("execution identity");
        }
        let covered = apply_coverage(
            &mut plan,
            &self.manifest,
            &provenance_for(&self.manifest),
            discovery,
            Some(changed),
            &inputs(root, &catalog),
        );
        (covered, plan.warnings)
    }
}
