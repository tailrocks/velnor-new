//! Generator identity resolution and baseline coverage application.

use std::collections::BTreeSet;

use velnor_actions_contract::{BaselineProof, ObligationDecision, Plan};
use velnor_actions_mise::catalog::lock::{load_text, parse_generator_lock};

use crate::discover::Discovery;
use crate::extension_schemas::coverage_schema_known;
use crate::external_data::{
    DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, external_data_kind, may_skip_external_data,
};
use crate::internal::plan_obligation::{changed_keys, member_changed};
use crate::internal_plan::extension_bundle;
use crate::merge::BaselineManifest;

/// Bootstrap lock resolving release generator identity for source builds.
const GENERATOR_LOCK_REL: &str = ".velnor/generator.lock";

/// Lookup-skipped reason for an unverifiable source-build generator.
pub(crate) const SOURCE_BUILD_REASON: &str = "generator_unverifiable_source_build";

/// True for generator SHAs that prove nothing: all-zero or empty.
///
/// An all-zero SHA is the source-build marker: no release binary stands
/// behind it, so baseline evidence bound to it is unverifiable.
pub(crate) fn is_source_build(sha: &str) -> bool {
    sha.is_empty() || (sha.len() == 64 && sha.bytes().all(|b| b == b'0'))
}

/// Fill an unverifiable generator SHA from the release lock when it pins
/// this exact version and target; otherwise keep the source-build marker.
///
/// Explicit caller-supplied SHAs win over the lock; any unreadable or
/// mismatched lock quietly leaves the marker, skipping live lookup.
pub(crate) fn resolve_generator_identity(plan: &mut Plan, root: &std::path::Path) {
    if !is_source_build(&plan.generator.sha256) {
        return;
    }
    let path = root.join(GENERATOR_LOCK_REL);
    if !path.is_file() {
        return;
    }
    let Ok(text) = load_text(&path) else {
        return;
    };
    let Ok(lock) = parse_generator_lock(&text) else {
        return;
    };
    let Some(target) = release_target(&plan.generator.target) else {
        return;
    };
    if lock.generator.version != plan.generator.version {
        return;
    }
    if let Some(record) = lock.binary_for_target(target) {
        plan.generator.sha256.clone_from(&record.sha256);
    }
}

/// True when the adapter extension refuses baseline coverage (PAR-4.10).
///
/// Undeclared reads and conservative execution both force execution;
/// obligations without adapter inputs keep the other coverage checks.
fn coverage_refused(discovery: &Discovery, task_id: &str) -> bool {
    let Some(group) = discovery
        .task_groups
        .iter()
        .find(|group| group.task_id == task_id)
    else {
        return false;
    };
    let bundle = extension_bundle(discovery, group);
    let ext = group.identity_extension(&bundle.inputs());
    ext.coverage_eligible().is_err() || ext.conservative_execution_required()
}

/// Release triple for a generator target: triples pass through, else map.
///
/// Source builds record `{arch}-{os}`; release locks pin full triples.
fn release_target(target: &str) -> Option<&str> {
    if velnor_actions_contract::SUPPORTED_TARGETS.contains(&target) {
        return Some(target);
    }
    match target {
        "x86_64-linux" => Some("x86_64-unknown-linux-gnu"),
        "aarch64-macos" => Some("aarch64-apple-darwin"),
        "x86_64-macos" => Some("x86_64-apple-darwin"),
        _ => None,
    }
}

/// Mark covered obligations and prune the matrix; returns covered count.
///
/// Changed obligations never cover, even on identity match: the
/// changed hint guards identities that may miss semantic inputs.
/// Baseline provenance is set by the caller from the returned count.
pub(crate) fn apply_coverage(
    plan: &mut Plan,
    manifest: &BaselineManifest,
    digest: &str,
    discovery: &Discovery,
    changed: Option<&BTreeSet<String>>,
) -> u32 {
    let universe: Vec<_> = discovery.task_groups.iter().collect();
    let keys = changed
        .map(|set| changed_keys(&universe, set))
        .unwrap_or_default();
    let mut covered = 0u32;
    for obligation in &mut plan.obligations {
        let group = discovery
            .task_groups
            .iter()
            .find(|group| group.task_id == obligation.task_id);
        if group.is_none_or(|group| member_changed(group, changed, &keys)) {
            continue;
        }
        let hit = manifest.tasks.iter().find(|task| {
            task.task_id == obligation.task_id
                && task.task_digest == obligation.task_digest
                && task.input_digest == obligation.input_digest
        });
        let Some(task) = hit else {
            plan.warnings
                .push(format!("baseline_miss:{}:no_entry", obligation.task_id));
            continue;
        };
        if coverage_refused(discovery, &obligation.task_id) {
            plan.warnings.push(format!(
                "baseline_miss:{}:undeclared_inputs",
                obligation.task_id
            ));
            continue;
        }
        if !coverage_schema_known(&obligation.task_id) {
            plan.warnings.push(format!(
                "baseline_miss:{}:unknown_extension_schema",
                obligation.task_id
            ));
            continue;
        }
        if external_data_kind(&obligation.task_id).is_some()
            && !may_skip_external_data(
                true,
                task.external_data.as_ref(),
                DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS,
            )
        {
            plan.warnings.push(format!(
                "baseline_miss:{}:external_data_rerun",
                obligation.task_id
            ));
            continue;
        }
        obligation.decision = ObligationDecision::CoveredByTrustedBaseline;
        obligation.reason = String::from("covered_by_trusted_baseline");
        obligation.baseline_proof = Some(BaselineProof {
            source_commit: manifest.source_commit.clone(),
            run_id: task.proof_run_id,
            artifact_id: manifest.artifact_id,
            artifact_name: manifest.artifact_name.clone(),
            manifest_digest: digest.to_owned(),
        });
        covered += 1;
    }
    plan.matrix.include.retain(|entry| {
        plan.obligations
            .iter()
            .any(|ob| ob.task_id == entry.task_id && ob.decision == ObligationDecision::Execute)
    });
    for package in &mut plan.packages {
        package.selected = plan.obligations.iter().any(|ob| {
            ob.decision == ObligationDecision::Execute && package.tasks.contains(&ob.task_id)
        });
    }
    covered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_data::ExternalDataFreshness;
    use crate::merge::BaselineManifest;
    use crate::merge::required_evidence::BaselineTaskEntry;
    use velnor_actions_contract::{
        BaselineStatus, ObligationDecision, PlanBaseline, PlanGenerator, PlanMatrix,
        PlanObligation, PlanRunner, RunnerSelection, Trust, WorkflowEvent, digest_b3,
    };

    /// Plan carrying one execute obligation per `task_ids`.
    fn plan_with(task_ids: &[&str]) -> Plan {
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
            baseline: PlanBaseline {
                status: BaselineStatus::Unavailable,
                base_commit: None,
                run_id: None,
                artifact_id: None,
                artifact_name: None,
                manifest_digest: None,
                reason: None,
            },
            generator: PlanGenerator {
                version: "0.1.0".to_owned(),
                target: "x86_64-linux".to_owned(),
                sha256: "0".repeat(64),
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

    /// Manifest binding every `task_ids` entry, with advisory freshness.
    fn manifest_with(
        task_ids: &[&str],
        external_data: Option<&ExternalDataFreshness>,
    ) -> BaselineManifest {
        let digest = digest_b3(b"digest");
        BaselineManifest {
            schema: 1,
            repository_id: digest.clone(),
            source_commit: "a".repeat(40),
            ref_: "refs/heads/testmain".to_owned(),
            event: "push".to_owned(),
            workflow_ref: "o/r/.github/workflows/velnor.yml@refs/heads/testmain".to_owned(),
            run_id: 7,
            run_attempt: 1,
            final_status: "passed".to_owned(),
            generator_version: "0.1.0".to_owned(),
            generator_sha256: "1".repeat(64),
            compatibility_id: digest.clone(),
            artifact_id: 9,
            artifact_name: "velnor-baseline".to_owned(),
            expires_at_unix: None,
            tasks: task_ids
                .iter()
                .map(|id| BaselineTaskEntry {
                    task_id: (*id).to_owned(),
                    task_digest: digest.clone(),
                    input_digest: digest.clone(),
                    proof_run_id: 7,
                    observed_run_id: 7,
                    external_data: external_data.cloned(),
                    proof: None,
                })
                .collect(),
        }
    }

    /// Discovery with one plain group per task ID, all unchanged.
    fn discovery_with(task_ids: &[&str]) -> Discovery {
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
                    test_runner: "cargo_nextest".to_owned(),
                    declared_inputs: Vec::new(),
                    undeclared_reads: false,
                    uses_network: false,
                    uses_clock: false,
                    uses_random: false,
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

    /// Fresh advisory proof over a fixed identity.
    fn fresh_proof() -> ExternalDataFreshness {
        ExternalDataFreshness {
            source: "advisory-db".to_owned(),
            identity: digest_b3(b"snapshot"),
            age_secs: 60,
        }
    }

    #[test]
    fn unknown_schema_and_stale_external_data_stay_execute() {
        let rust = "stack/rust/root/clippy/default";
        let unknown = "stack/unknown/root/test/default";
        let advisory = "stack/rust/root/advisory/default";
        let mut plan = plan_with(&[rust, unknown, advisory]);
        let manifest = manifest_with(&[rust, unknown, advisory], None);
        let unchanged = Some(BTreeSet::new());
        let covered = apply_coverage(
            &mut plan,
            &manifest,
            &digest_b3(b"m"),
            &discovery_with(&[rust, unknown, advisory]),
            unchanged.as_ref(),
        );
        assert_eq!(covered, 1);
        let decision = |id: &str| {
            plan.obligations
                .iter()
                .find(|ob| ob.task_id == id)
                .map(|ob| ob.decision)
        };
        assert_eq!(
            decision(rust),
            Some(ObligationDecision::CoveredByTrustedBaseline)
        );
        assert_eq!(decision(unknown), Some(ObligationDecision::Execute));
        assert_eq!(decision(advisory), Some(ObligationDecision::Execute));
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.contains("unknown_extension_schema"))
        );
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.contains("external_data_rerun"))
        );
    }

    #[test]
    fn fresh_external_data_covers_advisory() {
        let advisory = "stack/rust/root/advisory/default";
        let mut plan = plan_with(&[advisory]);
        let proof = fresh_proof();
        let manifest = manifest_with(&[advisory], Some(&proof));
        let unchanged = Some(BTreeSet::new());
        let covered = apply_coverage(
            &mut plan,
            &manifest,
            &digest_b3(b"m"),
            &discovery_with(&[advisory]),
            unchanged.as_ref(),
        );
        assert_eq!(covered, 1);
        assert_eq!(
            plan.obligations[0].decision,
            ObligationDecision::CoveredByTrustedBaseline
        );
        assert!(plan.obligations[0].baseline_proof.is_some());
    }
}
